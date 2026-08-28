//! Session registry and the per-session actor.
//!
//! Each session owns its state on a single task and is reached only through an
//! `mpsc` channel. Nothing else touches a `Market`, so there is no lock to
//! contend on and no chance of two commands interleaving inside the engine.
//! That is also what makes the sequence numbers meaningful: the actor is the
//! only thing that increments them.
//!
//! Fanout is a `broadcast` channel. Every connection subscribes; events that are
//! private to one connection (rejections, snapshots) go back down that
//! connection's own reply channel instead.
//!
//! Note there are two sequence counters, deliberately. `Market::seq` is queue
//! priority inside the engine. `SessionActor::seq` is the browser's event
//! stream. They answer different questions and must not be merged.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use engine::{Command, Market, Price, Reject};
use rand::Rng;
use rusqlite::Connection;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};

use crate::db;
use crate::translate::to_engine_command;
use crate::protocol::{self, *};

/// Who is on the other end of a connection. The host does not trade.
#[derive(Debug, Clone)]
pub enum Who {
    Host,
    Player { id: String, name: String },
}

pub struct Envelope {
    pub who: Who,
    pub cmd: ClientCommand,
    /// Private channel back to the one connection that sent this.
    pub reply: mpsc::UnboundedSender<ServerEvent>,
}

pub enum Msg {
    Cmd(Envelope),
    Join {
        name: String,
        reply: oneshot::Sender<(String, String)>, // (player_id, player_token)
    },
    Meta(oneshot::Sender<SessionMeta>),
    /// Resolve a player token back to a seat, so a refresh restores it.
    Resolve {
        token: String,
        reply: oneshot::Sender<Option<Player>>,
    },
    /// Render the tape as CSV for the host's download.
    Export(oneshot::Sender<String>),
    /// A stable description of live market state, for checking it against a
    /// replay of the command log.
    Fingerprint(oneshot::Sender<String>),
    /// Wake the bots. Sent on a timer; does nothing if there are none.
    BotTick,
}

pub struct SessionHandle {
    pub code: String,
    pub host_token: String,
    pub tx: mpsc::Sender<Msg>,
    pub events: broadcast::Sender<ServerEvent>,
    /// Epoch millis of the last thing anyone did here. Read by the idle sweeper
    /// without going near the actor, so a wedged session can still be reaped.
    last_activity: AtomicI64,
}

impl SessionHandle {
    /// Record that something happened. Call this on every inbound command.
    pub fn touch(&self) {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
    }

    pub fn idle_ms(&self) -> i64 {
        now_ms() - self.last_activity.load(Ordering::Relaxed)
    }

    pub async fn meta(&self) -> Option<SessionMeta> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Msg::Meta(tx)).await.ok()?;
        rx.await.ok()
    }

    pub async fn join(&self, name: String) -> Option<(String, String)> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Msg::Join { name, reply: tx }).await.ok()?;
        rx.await.ok()
    }

    pub async fn resolve(&self, token: String) -> Option<Player> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Msg::Resolve { token, reply: tx }).await.ok()?;
        rx.await.ok().flatten()
    }

    pub async fn export(&self) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Msg::Export(tx)).await.ok()?;
        rx.await.ok()
    }

    pub async fn fingerprint(&self) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Msg::Fingerprint(tx)).await.ok()?;
        rx.await.ok()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub sessions: Arc<Mutex<HashMap<String, Arc<SessionHandle>>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState { sessions: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub async fn get(&self, code: &str) -> Option<Arc<SessionHandle>> {
        self.sessions.lock().await.get(&code.to_uppercase()).cloned()
    }

    /// Spawn a new session actor and register it.
    pub async fn create(&self, req: CreateSessionRequest) -> Arc<SessionHandle> {
        let code = generate_code();
        let host_token = uuid::Uuid::new_v4().to_string();

        let (tx, rx) = mpsc::channel::<Msg>(256);
        let (events, _) = broadcast::channel::<ServerEvent>(1024);

        let market = Market::new(engine::Config {
            tick: req.tick_size.map(Price::from_f64),
            position_limit: req.position_limit,
        });

        let meta = SessionMeta {
            code: code.clone(),
            question: req.question,
            unit: req.unit,
            tick_size: req.tick_size,
            position_limit: req.position_limit,
            phase: Phase::Lobby,
        };

        // Persistence is best-effort: if the database cannot be opened the game
        // still runs, it just is not recoverable. Losing the round to a failed
        // open would be a worse trade in a pub.
        let db = db::open(&db_path())
            .map_err(|e| tracing::warn!("sqlite unavailable, running without persistence: {e}"))
            .ok();
        if let Some(conn) = &db {
            let _ = db::insert_session(
                conn,
                &meta.code,
                &meta.question,
                &meta.unit,
                meta.tick_size,
                meta.position_limit,
                now_ms(),
            );
        }

        let actor = SessionActor {
            meta,
            players: Vec::new(),
            names: HashMap::new(),
            tokens: HashMap::new(),
            market,
            bots: Vec::new(),
            bot_rate: 6.0,
            bot_buy_bias: 0.5,
            trades: Vec::new(),
            settlement: None,
            seq: 0,
            log_seq: 0,
            events: events.clone(),
            db,
        };

        tokio::spawn(actor.run(rx));

        // Wake the bots on a timer. This holds a *weak* sender, so it does not
        // keep the actor alive: once the registry drops the session, the upgrade
        // fails and this task exits with it.
        let weak = tx.downgrade();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(std::time::Duration::from_millis(900));
            loop {
                ticks.tick().await;
                let Some(tx) = weak.upgrade() else { break };
                if tx.send(Msg::BotTick).await.is_err() {
                    break;
                }
            }
        });

        let handle = Arc::new(SessionHandle {
            code: code.clone(),
            host_token,
            tx,
            events,
            last_activity: AtomicI64::new(now_ms()),
        });
        self.sessions.lock().await.insert(code, handle.clone());
        handle
    }
}

impl AppState {
    /// Drop sessions nobody has touched for `ttl_ms`.
    ///
    /// Dropping the handle drops the only `mpsc::Sender`, so the actor's
    /// `recv()` returns `None` and the task exits on its own. Live connections
    /// hold their own `Arc`, so a session with anyone still attached survives
    /// until they leave.
    pub async fn evict_idle(&self, ttl_ms: i64) -> usize {
        let mut sessions = self.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|_, h| h.idle_ms() < ttl_ms);
        before - sessions.len()
    }
}

pub fn db_path() -> String {
    std::env::var("DB_PATH").unwrap_or_else(|_| "open_outcry.db".to_string())
}

/// Six characters from an alphabet with no 0/O/1/I, because these get read off a
/// projector in a dim room.
fn generate_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..6).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

/// A bot.
///
/// Bots are **customers, not market makers**. They never post a quote — they
/// only lift offers and hit bids that the humans have made.
///
/// They also have no view on price. They buy and sell at random, at a rate the
/// host sets. That is deliberate: the point of them is to supply flow so a
/// small room still has someone to trade against, and giving them opinions
/// would quietly turn them into the thing the players are supposed to be
/// competing at.
#[derive(Debug, Clone)]
struct Bot {
    id: String,
}

/// How often the bot timer fires. Bot rates are set per minute and converted
/// against this.
const TICK_MS: f64 = 900.0;

const BOT_NAMES: [&str; 12] = [
    "Ada", "Bo", "Cleo", "Dex", "Eve", "Finn", "Gus", "Hana", "Ivo", "Jax", "Kit", "Lux",
];

struct SessionActor {
    meta: SessionMeta,
    players: Vec<Player>,
    /// player_id -> display name, so book and tape entries can carry names.
    names: HashMap<String, String>,
    /// player_token -> player_id
    tokens: HashMap<String, String>,
    market: Market,
    bots: Vec<Bot>,
    /// Orders per minute, per bot.
    bot_rate: f64,
    /// 0..1. The chance any given bot order is a buy rather than a sell.
    bot_buy_bias: f64,
    trades: Vec<Trade>,
    settlement: Option<Settlement>,
    /// Broadcast sequence. Separate from `market.seq`.
    seq: u64,
    /// Command-log sequence. Also separate: `market.seq` does not advance on a
    /// cancel or a phase change, so using it as the log's key would collide.
    log_seq: u64,
    events: broadcast::Sender<ServerEvent>,
    db: Option<Connection>,
}

impl SessionActor {
    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Meta(reply) => {
                    let _ = reply.send(self.meta.clone());
                }
                Msg::Resolve { token, reply } => {
                    let found = self
                        .tokens
                        .get(&token)
                        .and_then(|id| self.players.iter().find(|p| &p.id == id))
                        .cloned();
                    let _ = reply.send(found);
                }
                Msg::Join { name, reply } => {
                    let (id, token) = self.add_player(name, false);
                    let _ = reply.send((id, token));
                }
                Msg::Export(reply) => {
                    let _ = reply.send(self.to_csv());
                }
                Msg::Fingerprint(reply) => {
                    let _ = reply.send(self.market.fingerprint());
                }
                Msg::BotTick => self.bot_tick(),
                Msg::Cmd(env) => self.handle(env),
            }
        }
    }

    fn add_player(&mut self, name: String, is_bot: bool) -> (String, String) {
        let player = Player {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.clone(),
            joined_at: now_ms(),
            is_bot,
        };
        let token = uuid::Uuid::new_v4().to_string();

        // The engine needs to know about the player before it will accept
        // orders from them.
        let _ = self.market.apply(Command::AddPlayer { player: engine::PlayerId(player.id.clone()) });

        self.tokens.insert(token.clone(), player.id.clone());
        self.names.insert(player.id.clone(), name);
        self.players.push(player.clone());

        if let Some(conn) = &self.db {
            let _ = db::insert_player(conn, &self.meta.code, &player.id, &player.name, player.joined_at);
        }

        self.seq += 1;
        let _ = self.events.send(ServerEvent::PlayerJoined { seq: self.seq, player: player.clone() });
        (player.id, token)
    }

    fn name_of(&self, id: &str) -> String {
        self.names.get(id).cloned().unwrap_or_else(|| "?".to_string())
    }

    fn to_wire_order(&self, o: &engine::Order) -> protocol::Order {
        protocol::Order {
            id: o.id.0.to_string(),
            player_id: o.player.0.clone(),
            player_name: self.name_of(&o.player.0),
            side: match o.side {
                engine::Side::Bid => Side::Bid,
                engine::Side::Offer => Side::Offer,
            },
            price: o.price.to_f64(),
            seq: o.seq,
        }
    }

    /// Flatten the engine's book, best price first on each side. `BTreeMap`
    /// iterates ascending, so bids are reversed and offers are not; within a
    /// price level the queue is already in arrival order.
    fn wire_book(&self) -> BookState {
        BookState {
            bids: self
                .market
                .book
                .bids
                .iter()
                .rev()
                .flat_map(|(_, q)| q.iter())
                .map(|o| self.to_wire_order(o))
                .collect(),
            offers: self
                .market
                .book
                .offers
                .iter()
                .flat_map(|(_, q)| q.iter())
                .map(|o| self.to_wire_order(o))
                .collect(),
        }
    }

    fn position_of(&self, player_id: &str) -> i64 {
        self.market
            .positions
            .get(&engine::PlayerId(player_id.to_string()))
            .map_or(0, |p| p.net)
    }

    fn snapshot(&self, who: &Who) -> ServerEvent {
        ServerEvent::Snapshot {
            seq: self.seq,
            session: self.meta.clone(),
            players: self.players.clone(),
            book: self.wire_book(),
            trades: self.trades.clone(),
            you: match who {
                Who::Host => None,
                Who::Player { id, name } => Some(YouState {
                    player_id: id.clone(),
                    name: name.clone(),
                    position: self.position_of(id),
                }),
            },
            settlement: self.settlement.clone(),
        }
    }

    fn handle(&mut self, env: Envelope) {
        let is_host = matches!(env.who, Who::Host);

        match &env.cmd {
            ClientCommand::Resync { .. } => {
                let _ = env.reply.send(self.snapshot(&env.who));
            }

            ClientCommand::OpenTrading
            | ClientCommand::CloseTrading
            | ClientCommand::Settle { .. }
            | ClientCommand::AddBot
            | ClientCommand::SetBotFlow { .. }
            | ClientCommand::RemoveBots
                if !is_host =>
            {
                let _ = env.reply.send(ServerEvent::Rejected {
                    reason: RejectReason::NotHost,
                    message: "Only the host can do that".into(),
                });
            }
            ClientCommand::OpenTrading => {
                if self.set_phase(Phase::Open) {
                    self.persist_command(&env.cmd, None);
                }
            }
            ClientCommand::CloseTrading => {
                if self.set_phase(Phase::Closed) {
                    self.persist_command(&env.cmd, None);
                }
            }
            ClientCommand::Settle { true_value } => {
                self.settle(*true_value);
                self.persist_command(&env.cmd, None);
            }
            ClientCommand::AddBot => self.add_bot(),
            ClientCommand::SetBotFlow { orders_per_minute, buy_bias } => {
                self.bot_rate = orders_per_minute.clamp(0.0, 60.0);
                self.bot_buy_bias = buy_bias.clamp(0.0, 1.0);
            }
            ClientCommand::RemoveBots => self.bots.clear(),

            ClientCommand::PlaceOrder { .. }
            | ClientCommand::CancelOrder { .. }
            | ClientCommand::Take { .. } => {
                let Who::Player { id, .. } = &env.who else {
                    let _ = env.reply.send(ServerEvent::Rejected {
                        reason: RejectReason::NotHost,
                        message: "The host does not trade".into(),
                    });
                    return;
                };
                let cmd = match to_engine_command(Some(id), &env.cmd) {
                    Some(cmd) => cmd,
                    None => {
                        let _ = env.reply.send(ServerEvent::Error {
                            message: "unrecognised order".into(),
                        });
                        return;
                    }
                };
                self.run_engine(cmd, Some(id.clone()), &env)
            }
        }
    }

    /// Run a command through the engine, then broadcast whatever it produced.
    ///
    /// The engine decides *what* happened; the actor decides what the room is
    /// told and in what order. Rejections go back to the one player who caused
    /// them, never to the room.
    fn run_engine(&mut self, cmd: Command, actor: Option<String>, env: &Envelope) {
        if let Err(reject) = self.execute_command(cmd, actor.as_deref(), &env.cmd) {
            let (reason, message) = describe(reject);
            let _ = env.reply.send(ServerEvent::Rejected { reason, message: message.into() });
        }
    }

    /// Apply a command, and if the engine accepts it, log it and tell the room.
    ///
    /// Bots go through here too, under their own player id, so their orders are
    /// in the command log as concrete decisions rather than as a seed nobody
    /// could reproduce. That is what keeps a session with bots in it replayable.
    fn execute_command(
        &mut self,
        cmd: Command,
        actor: Option<&str>,
        wire: &ClientCommand,
    ) -> std::result::Result<(), Reject> {
        let events = self.market.apply(cmd)?;
        self.persist_command(wire, actor);
        for ev in events {
            self.publish(ev);
        }
        Ok(())
    }

    /// Turn one engine event into one broadcast event, stamped with the next
    /// sequence number.
    fn publish(&mut self, ev: engine::Event) {
        self.seq += 1;
        let seq = self.seq;

        let out = match ev {
            engine::Event::OrderAdded { order } => {
                ServerEvent::OrderAdded { seq, order: self.to_wire_order(&order) }
            }
            engine::Event::OrderCancelled { order, player } => ServerEvent::OrderCancelled {
                seq,
                order_id: order.0.to_string(),
                player_id: player.0,
            },
            engine::Event::Traded { trade } => {
                let wire = Trade {
                    id: format!("{}-{}", self.meta.code, trade.seq),
                    seq,
                    price: trade.price.to_f64(),
                    buyer_id: trade.buyer.0.clone(),
                    buyer_name: self.name_of(&trade.buyer.0),
                    seller_id: trade.seller.0.clone(),
                    seller_name: self.name_of(&trade.seller.0),
                    aggressor: match trade.aggressor {
                        engine::Direction::Buy => Direction::Buy,
                        engine::Direction::Sell => Direction::Sell,
                    },
                    self_trade: trade.self_trade,
                    ts: now_ms(),
                };
                self.trades.push(wire.clone());
                if let Some(conn) = &self.db {
                    let _ = db::insert_trade(
                        conn,
                        &self.meta.code,
                        &wire.id,
                        seq,
                        wire.price,
                        &wire.buyer_id,
                        &wire.seller_id,
                        match wire.aggressor {
                            Direction::Buy => "buy",
                            Direction::Sell => "sell",
                        },
                        wire.self_trade,
                        wire.ts,
                    );
                }
                ServerEvent::Trade {
                    seq,
                    trade: wire,
                    resting_order_id: trade.resting_order.0.to_string(),
                }
            }
            engine::Event::PhaseChanged { phase } => {
                let p = wire_phase(phase);
                self.meta.phase = p;
                ServerEvent::PhaseChanged { seq, phase: p }
            }
            // Settlement is broadcast by `settle`, which has the results.
            engine::Event::Settled { .. } | engine::Event::PlayerAdded { .. } => return,
        };

        let _ = self.events.send(out);
    }

    /// Returns whether the engine accepted the transition.
    fn add_bot(&mut self) {
        let n = self.bots.len();
        let base = BOT_NAMES[n % BOT_NAMES.len()];
        let name = if n < BOT_NAMES.len() {
            base.to_string()
        } else {
            format!("{base}{}", n / BOT_NAMES.len() + 1)
        };
        let (id, _token) = self.add_player(name, true);
        self.bots.push(Bot { id });
    }

    /// One roll per bot, on a timer.
    ///
    /// No pricing, no opinion, no cleverness: each bot decides whether to act
    /// at all, then picks a direction, then takes whatever price is there. If
    /// the side it wants is empty it simply does nothing this tick.
    fn bot_tick(&mut self) {
        if self.bots.is_empty() || self.meta.phase != Phase::Open {
            return;
        }

        // Orders per minute, as a per-tick probability. Capped at 1 so a very
        // high rate cannot make a bot act more than once per tick.
        let per_tick = (self.bot_rate * TICK_MS / 60_000.0).clamp(0.0, 1.0);
        if per_tick <= 0.0 {
            return;
        }
        let buy_bias = self.bot_buy_bias;

        for bot in self.bots.clone() {
            let mut rng = rand::thread_rng();
            if rng.gen_range(0.0..1.0) >= per_tick {
                continue;
            }

            let direction = if rng.gen_range(0.0..1.0) < buy_bias {
                Direction::Buy
            } else {
                Direction::Sell
            };

            // Nothing to hit on that side. The bot does not go looking for the
            // other one — that would quietly reintroduce a preference.
            let available = match direction {
                Direction::Buy => self.market.book.best_offer().is_some(),
                Direction::Sell => self.market.book.best_bid().is_some(),
            };
            if !available {
                continue;
            }

            let wire = ClientCommand::Take { direction };
            if let Some(cmd) = to_engine_command(Some(&bot.id), &wire) {
                // Rejections are expected: the bot may be at its position limit,
                // or another bot may have just taken the price it wanted.
                let _ = self.execute_command(cmd, Some(&bot.id), &wire);
            }
        }
    }

    fn set_phase(&mut self, phase: Phase) -> bool {
        let engine_phase = match phase {
            Phase::Lobby => engine::Phase::Lobby,
            Phase::Open => engine::Phase::Open,
            Phase::Closed => engine::Phase::Closed,
            Phase::Settled => engine::Phase::Settled,
        };
        let Ok(events) = self.market.apply(Command::SetPhase { phase: engine_phase }) else {
            return false;
        };
        if let Some(conn) = &self.db {
            let _ = db::set_phase(conn, &self.meta.code, phase_name(phase));
        }
        for ev in events {
            self.publish(ev);
        }
        true
    }

    fn settle(&mut self, true_value: f64) {
        let tv = Price::from_f64(true_value);
        let _ = self.market.apply(Command::Settle { true_value: tv });
        self.meta.phase = Phase::Settled;

        // Bots are left off the leaderboard: it is a scoreboard for the room,
        // and nobody wants to be beaten by Cleo. Note this means the visible
        // P&L no longer sums to zero — the bots hold the other side of it.
        let results: Vec<protocol::Result> = self
            .players
            .iter()
            .filter(|p| !p.is_bot)
            .map(|p| {
                let id = engine::PlayerId(p.id.clone());
                let pos = self.market.positions.get(&id).cloned().unwrap_or_default();
                protocol::Result {
                    player_id: p.id.clone(),
                    name: p.name.clone(),
                    position: pos.net,
                    cash: pos.cash as f64 / Price::SCALE as f64,
                    pnl: self.market.settle_pnl(&id, tv) as f64 / Price::SCALE as f64,
                }
            })
            .collect();

        self.settlement = Some(Settlement { true_value, results: results.clone() });
        if let Some(conn) = &self.db {
            let _ = db::set_settlement(conn, &self.meta.code, true_value);
        }

        self.seq += 1;
        let _ = self.events.send(ServerEvent::Settled { seq: self.seq, true_value, results });
    }

    /// Append an accepted command to the replay log.
    ///
    /// Only accepted commands go in. A rejected command changed nothing, so
    /// replaying it would be replaying a decision the engine already made.
    fn persist_command(&mut self, cmd: &ClientCommand, player_id: Option<&str>) {
        self.log_seq += 1;
        let Some(conn) = &self.db else { return };
        let Ok(payload) = serde_json::to_string(cmd) else { return };
        let _ =
            db::append_command(conn, &self.meta.code, self.log_seq, player_id, &payload, now_ms());
    }

    fn to_csv(&self) -> String {
        let mut out = String::from("seq,ts,price,buyer,seller,aggressor,self_trade\n");
        for t in &self.trades {
            out.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                t.seq,
                t.ts,
                t.price,
                csv_escape(&t.buyer_name),
                csv_escape(&t.seller_name),
                match t.aggressor {
                    Direction::Buy => "buy",
                    Direction::Sell => "sell",
                },
                t.self_trade
            ));
        }
        out
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn wire_phase(p: engine::Phase) -> Phase {
    match p {
        engine::Phase::Lobby => Phase::Lobby,
        engine::Phase::Open => Phase::Open,
        engine::Phase::Closed => Phase::Closed,
        engine::Phase::Settled => Phase::Settled,
    }
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Lobby => "lobby",
        Phase::Open => "open",
        Phase::Closed => "closed",
        Phase::Settled => "settled",
    }
}

/// Engine rejections, in words a player in a pub can act on.
fn describe(r: Reject) -> (RejectReason, &'static str) {
    match r {
        Reject::PositionLimit => (RejectReason::PositionLimit, "Position limit reached"),
        Reject::NotOpen => (RejectReason::NotOpen, "Trading is not open"),
        Reject::NoLiquidity => (RejectReason::NoLiquidity, "Nothing to trade against"),
        Reject::BadTick => (RejectReason::BadTick, "Price is not on the tick"),
        Reject::BadPrice => (RejectReason::BadPrice, "That is not a valid price"),
        Reject::UnknownOrder => (RejectReason::UnknownOrder, "That order is already gone"),
        Reject::NotYourOrder => (RejectReason::NotYourOrder, "That is not your order"),
        Reject::UnknownPlayer => (RejectReason::UnknownPlayer, "You are not in this session"),
    }
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
