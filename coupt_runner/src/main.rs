use coup::{
	bots::{HonestBot, RandomBot, StaticBot, BluffingBot},
	Coup,
};

fn main() {
	let mut coup_game = Coup::new(vec![
		Box::new(StaticBot),
		Box::new(HonestBot),
		Box::new(HonestBot),
		Box::new(RandomBot),
		Box::new(BluffingBot)
	]);

	coup_game.play();
}