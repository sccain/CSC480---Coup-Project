use coup::{
    bots::{HonestBot, RandomBot, StaticBot},
    Coup,
};

fn main() {
    let mut coup_game = Coup::new(vec![
        Box::new(StaticBot),
        Box::new(HonestBot),
        Box::new(RandomBot),
        Box::new(StaticBot),
        Box::new(RandomBot),
        Box::new(HonestBot),
    ]);

    // Play a single game
    coup_game.play();

    // Or play multiple games
    // coup_game.looping(20);
}
