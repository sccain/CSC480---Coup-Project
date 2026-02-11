use coup::{
    bots::{HonestBot, RandomBot, StaticBot, DuelBot},
    Coup,
};

fn main() {
    let mut coup_game = Coup::new(vec![
        Box::new(DuelBot),
        Box::new(HonestBot)
    ]);

    // Play a single game
    coup_game.play();

    // Or play multiple games
    //coup_game.looping(50);
}
