//! An honest bot implementation for you to use to test your own bot with.
use std::io;
use crate::{
	bot::{BotInterface, Context},
	Action, Card,
};

/// The human should be able to decide what happens on each turn
pub struct Human;

impl BotInterface for Human {
	/// Human is the name
	fn get_name(&self) -> String {
		String::from("Human")
	}

	/// Acts on cards it has and falls back to [Action::Income].
	/// Never plays [Action::ForeignAid] or [Action::Swapping].
	fn on_turn(&self, context: &Context) -> Action {
        for card in context.cards.clone(){
            println!("{:#?}", card);
        }
    
        let mut action = 0;
        while true{
            let mut input = String::new();
            println!("Select an action: 
                    \n1. Income: Collect 1 coin
                    \n2. Foreign Aid: Collect 2 coins
                    \n3. Tax: Collect 3 coins as Duke
                    \n4. Steal: Take coins from another player as Captain
                    \n5. Exchange: Replace cards as Ambassador");

            if context.coins >= 3 {
                println!("6. Assassinate: Pay 3 coins to assasinate another player as Assasin");
            }
            if context.coins >= 7 {
                println!("7. Coup: Pay 7 coins to launch a Coup on another player");
            }
            print!("> ");
            
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            
            let num: i32 = input
                .trim() // Remove whitespace
                .parse() // Convert to i32
                .expect("Please enter a valid number");

            if num >= 1 && num <= 5 {
                action = num;
                break;
            } else if num >= 1 && num <= 6 && context.coins >= 3 {
                action = num;
                break;
            } else if num >= 1 && num <= 7 && context.coins >= 7 {
                action = num;
                break;
            }
            
            println!("Invalid selection or insufficient coins");
        }

        if action == 1 {
            Action::Income
        } else if action == 2 {
            Action::ForeignAid
        } else if action == 3 {
            Action::Tax
        } else if action == 5 {
            Action::Swapping
        } else  {
            let mut target = String::new();
            loop {
                println!("Select a target: ");
                let mut idx = 0;
                for bot in context.playing_bots.iter() {
                    idx = idx + 1;
                    println!("{}: {}", idx, bot.name);
                }
                print!("> ");
                io::stdin()
                    .read_line(&mut target)
                    .expect("Failed to read line");

                if context.playing_bots.iter().filter(|bot| bot.name == target).count() == 1 {
                    break;
                } else {
                    println!("Invalid bot");
                }
            }

            if action == 4 {
                Action::Stealing(target)
            } else if action == 6 {
                Action::Assassination(target)
            } else {
                Action::Coup(target)
            }
        }
	}

	fn on_auto_coup(&self, context: &Context) -> String {
		let mut target = String::new();
        loop {
            println!("Select a Coup target: ");
            let mut idx = 0;
            for bot in context.playing_bots.iter() {
                idx = idx + 1;
                println!("{}: {}", idx, bot.name);
            }
            print!("> ");
            io::stdin()
                .read_line(&mut target)
                .expect("Failed to read line");

            if context.playing_bots.iter().filter(|bot| bot.name == target).count() == 1 {
                break;
            } else {
                println!("Invalid bot");
            }
        }
        target
	}

	fn on_challenge_action_round(
		&self,
		action: &Action,
		_by: String,
		context: &Context,
	) -> bool {
		let mut choice = 0;
        loop {
            let mut input = String::new();
            println!("Challenge?
                    \n 0: No
                    \n 1: Yes");
            print!("> ");
            
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            
            let num: i32 = input
                .trim() // Remove whitespace
                .parse() // Convert to i32
                .expect("Please enter a valid number");

            if num == 0 {
                choice = num;
                break;
            } else if num == 1 {
                choice = num;
                break;
            }   
        } 
        if choice == 0 {
            false
        } else {
            true
        }
	}

	/// Counters only if it has the card to counter
	fn on_counter(
		&self,
		action: &Action,
		_by: String,
		context: &Context,
	) -> bool {
		let mut choice = 0;
        loop {
            let mut input = String::new();
            println!("Counter?
                    \n 0: No
                    \n 1: Yes");
            print!("> ");
            
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            
            let num: i32 = input
                .trim() // Remove whitespace
                .parse() // Convert to i32
                .expect("Please enter a valid number");

            if num == 0 {
                choice = num;
                break;
            } else if num == 1 {
                choice = num;
                break;
            }   
        } 
        if choice == 1 {
            true
        } else {
            false
        }
	}


	fn on_challenge_counter_round(
		&self,
		action: &Action,
		_by: String,
		context: &Context,
	) -> bool {
		let mut choice = 0;
        loop {
            let mut input = String::new();
            println!("Challenge Counter?
                    \n 0: No
                    \n 1: Yes");
            print!("> ");
            
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            
            let num: i32 = input
                .trim() // Remove whitespace
                .parse() // Convert to i32
                .expect("Please enter a valid number");

            if num == 0 {
                choice = num;
                break;
            } else if num == 1 {
                choice = num;
                break;
            }   
        } 
        if choice == 0 {
            false
        } else {
            true
        }
	}

     /// INCOMPLETE \/\/\/\/

	/// Swaps duplicate cards
	fn on_swapping_cards(
		&self,
		new_cards: [Card; 2],
		context: &Context,
	) -> [Card; 2] {
		let mut discard_cards = Vec::new();
		if context.cards[0] == context.cards[1] {
			discard_cards.push(context.cards[0])
		} else {
			discard_cards.push(new_cards[0]);
		}
		discard_cards.push(new_cards[1]);

		[discard_cards[0], discard_cards[1]]
	}


	/// Takes the first card to discard
	fn on_card_loss(&self, context: &Context) -> Card {
		for card in context.cards.clone(){
            println!("{:#?}", card);
        }

        let mut choice = 0;
        loop {
            let mut input = String::new();
            println!("Challenge?
                    \n 0: No
                    \n 1: Yes");
            print!("> ");
            
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            
            let num: i32 = input
                .trim() // Remove whitespace
                .parse() // Convert to i32
                .expect("Please enter a valid number");

            if num == 0 {
                choice = num;
                break;
            } else if num == 1 {
                choice = num;
                break;
            }   
        } 
        
        context.cards.clone().pop().unwrap()
	}
}
