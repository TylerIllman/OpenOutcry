//! Wire commands to engine commands.
//!
//! Shared by the live path and by replay, so a session replayed from its log
//! cannot diverge from the session that was played.

use engine::{Command, PlayerId, Price};

use crate::protocol::{ClientCommand, Direction, Side};

/// `player_id` is `None` for host commands. Returns `None` for anything that
/// never reaches the engine — `resync` is a transport concern, not a command.
pub fn to_engine_command(player_id: Option<&str>, cmd: &ClientCommand) -> Option<Command> {
    Some(match cmd {
        ClientCommand::PlaceOrder { side, price } => Command::PlaceOrder {
            player: PlayerId(player_id?.to_string()),
            side: match side {
                Side::Bid => engine::Side::Bid,
                Side::Offer => engine::Side::Offer,
            },
            price: Price::from_f64(*price),
        },
        ClientCommand::CancelOrder { order_id } => Command::CancelOrder {
            player: PlayerId(player_id?.to_string()),
            order: engine::OrderId(order_id.parse().ok()?),
        },
        ClientCommand::Take { direction } => Command::Take {
            player: PlayerId(player_id?.to_string()),
            direction: match direction {
                Direction::Buy => engine::Direction::Buy,
                Direction::Sell => engine::Direction::Sell,
            },
        },
        ClientCommand::OpenTrading => Command::SetPhase { phase: engine::Phase::Open },
        ClientCommand::CloseTrading => Command::SetPhase { phase: engine::Phase::Closed },
        ClientCommand::Settle { true_value } => {
            Command::Settle { true_value: Price::from_f64(*true_value) }
        }
        // Not engine commands. Bots act by issuing ordinary PlaceOrder / Take /
        // CancelOrder commands under their own player id, which is what keeps
        // the command log replayable despite the randomness.
        ClientCommand::Resync { .. }
        | ClientCommand::AddBot { .. }
        | ClientCommand::RemoveBots => return None,
    })
}
