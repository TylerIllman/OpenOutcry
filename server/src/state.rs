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

use std::collections::HashMap;
use std::sync::Arc;

use rand::Rng;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};

use crate::protocol::*;

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
}

pub struct SessionHandle {
    pub code: String,
    pub host_token: String,
    pub tx: mpsc::Sender<Msg>,
    pub events: broadcast::Sender<ServerEvent>,
}

impl SessionHandle {
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

        let actor = SessionActor {
            meta: SessionMeta {
                code: code.clone(),
                question: req.question,
                unit: req.unit,
                tick_size: req.tick_size,
                position_limit: req.position_limit,
                phase: Phase::Lobby,
            },
            players: Vec::new(),
            tokens: HashMap::new(),
            book: BookState::default(),
            trades: Vec::new(),
            settlement: None,
            seq: 0,
            events: events.clone(),
        };

        tokio::spawn(actor.run(rx));

        let handle = Arc::new(SessionHandle { code: code.clone(), host_token, tx, events });
        self.sessions.lock().await.insert(code, handle.clone());
        handle
    }
}

/// Six characters from an alphabet with no 0/O/1/I, because these get read off a
/// projector in a dim room.
fn generate_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..6).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

struct SessionActor {
    meta: SessionMeta,
    players: Vec<Player>,
    /// player_token -> player_id
    tokens: HashMap<String, String>,
    book: BookState,
    trades: Vec<Trade>,
    settlement: Option<Settlement>,
    seq: u64,
    events: broadcast::Sender<ServerEvent>,
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
                    let player = Player {
                        id: uuid::Uuid::new_v4().to_string(),
                        name,
                        joined_at: now_ms(),
                    };
                    let token = uuid::Uuid::new_v4().to_string();
                    self.tokens.insert(token.clone(), player.id.clone());
                    self.players.push(player.clone());

                    self.seq += 1;
                    let _ = self.events.send(ServerEvent::PlayerJoined {
                        seq: self.seq,
                        player: player.clone(),
                    });
                    let _ = reply.send((player.id, token));
                }
                Msg::Cmd(env) => self.handle(env),
            }
        }
    }

    fn snapshot(&self, who: &Who) -> ServerEvent {
        ServerEvent::Snapshot {
            seq: self.seq,
            session: self.meta.clone(),
            players: self.players.clone(),
            book: self.book.clone(),
            trades: self.trades.clone(),
            you: match who {
                Who::Host => None,
                Who::Player { id, name } => Some(YouState {
                    player_id: id.clone(),
                    name: name.clone(),
                    // Position is derivable from the tape, but the server stays
                    // authoritative because it also enforces the limit.
                    position: position_of(&self.trades, id),
                }),
            },
            settlement: self.settlement.clone(),
        }
    }

    fn handle(&mut self, env: Envelope) {
        let is_host = matches!(env.who, Who::Host);

        match env.cmd {
            ClientCommand::Resync { .. } => {
                let _ = env.reply.send(self.snapshot(&env.who));
            }

            // -- host controls -------------------------------------------------
            ClientCommand::OpenTrading | ClientCommand::CloseTrading | ClientCommand::Settle { .. }
                if !is_host =>
            {
                let _ = env.reply.send(ServerEvent::Rejected {
                    reason: RejectReason::NotHost,
                    message: "Only the host can do that".into(),
                });
            }
            ClientCommand::OpenTrading => self.set_phase(Phase::Open),
            ClientCommand::CloseTrading => self.set_phase(Phase::Closed),
            ClientCommand::Settle { true_value } => {
                // TODO(tyler): once the engine exists, compute real results from
                // Market::settle_pnl for every player. Cash and position come
                // from the engine, not from replaying the tape here.
                self.meta.phase = Phase::Settled;
                let results: Vec<Result> = self
                    .players
                    .iter()
                    .map(|p| {
                        let position = position_of(&self.trades, &p.id);
                        let cash = cash_of(&self.trades, &p.id);
                        Result {
                            player_id: p.id.clone(),
                            name: p.name.clone(),
                            position,
                            cash,
                            pnl: cash + position as f64 * true_value,
                        }
                    })
                    .collect();

                self.settlement = Some(Settlement { true_value, results: results.clone() });
                self.seq += 1;
                let _ = self.events.send(ServerEvent::Settled {
                    seq: self.seq,
                    true_value,
                    results,
                });
            }

            // -- trading -------------------------------------------------------
            ClientCommand::PlaceOrder { .. }
            | ClientCommand::CancelOrder { .. }
            | ClientCommand::Take { .. } => {
                if is_host {
                    let _ = env.reply.send(ServerEvent::Rejected {
                        reason: RejectReason::NotHost,
                        message: "The host does not trade".into(),
                    });
                    return;
                }
                // TODO(tyler): this is the seam. Translate the wire command into
                // an `engine::Command`, call `market.apply(...)`, then map the
                // returned `Vec<engine::Event>` onto ServerEvents and broadcast
                // them with fresh sequence numbers. Reject maps 1:1 onto
                // RejectReason.
                let _ = env.reply.send(ServerEvent::Rejected {
                    reason: RejectReason::NotOpen,
                    message: "Matching engine not implemented yet".into(),
                });
            }
        }
    }

    fn set_phase(&mut self, phase: Phase) {
        self.meta.phase = phase;
        self.seq += 1;
        let _ = self.events.send(ServerEvent::PhaseChanged { seq: self.seq, phase });
    }
}

fn position_of(trades: &[Trade], player_id: &str) -> i64 {
    trades.iter().fold(0, |acc, t| {
        acc + (t.buyer_id == player_id) as i64 - (t.seller_id == player_id) as i64
    })
}

fn cash_of(trades: &[Trade], player_id: &str) -> f64 {
    trades.iter().fold(0.0, |acc, t| {
        acc + if t.seller_id == player_id { t.price } else { 0.0 }
            - if t.buyer_id == player_id { t.price } else { 0.0 }
    })
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
