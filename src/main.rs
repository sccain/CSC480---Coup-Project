// running MCTSBot vs HonestBot for testing
// replace bots here to compare performance
use coup::{
    bots::{HonestBot, RandomBot, StaticBot, DuelBot, mcts_bot::MctsBot},
    Coup,
};

fn main() {
    let mut coup_game = Coup::new(vec![
        Box::new(MctsBot),
        Box::new(HonestBot)
    ]);    

    // Play a single game
    coup_game.play();

    // Or play multiple games
    //coup_game.looping(50);
}
