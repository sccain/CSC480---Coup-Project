// running MCTSBot vs HonestBot for testing
// replace bots here to compare performance
use coup::{


    bots::{Human, HonestBot, RandomBot, StaticBot, mcts_bot::MctsBot, duel_bot::DuelBot},
    Coup,
};

fn main() {
    let mut coup_game = Coup::new(vec![

        Box::new(MctsBot::default()),
        Box::new(HonestBot),
        Box::new(DuelBot),
        //Box::new(Human),
        //Box::new(HonestBot)
        Box::new(RandomBot)
    ]); 

    // Play a single game
    //coup_game.play();

    // Or play multiple games
    coup_game.looping(1000);
}
