//! An honest bot implementation for you to use to test your own bot with.
use std::io::{self, Write};
use crate::{
	bot::{BotInterface, Context},
	Action, Card,
};


/// Prints a compact reference of all roles
fn print_role_reference() {
    println!("\n");
    println!("Role Reference");
    println!("Duke       → Tax (3 coins), blocks Foreign Aid");
    println!("Assassin   → Assassinate (pay 3 coins)");
    println!("Captain    → Steal (2 coins), blocks Steal");
    println!("Ambassador → Exchange cards, blocks Steal");
    println!("Contessa   → Blocks Assassination");
    println!("\n");

}

/// The human should be able to decide what happens on each turn
pub struct Human;

impl BotInterface for Human {
	/// Human is the name
	fn get_name(&self) -> String {
		String::from("Human")
	}

    fn on_turn(&self, context: &Context) -> Action {
        print!("\n");

        for card in context.cards.clone() {
            println!("{:#?}", card);
        }
        print!("\n");
        println!("(Type 'h' for role reference)");

        // decide the maximum action number available based on coins
        let max_action = if context.coins >= 7 {
            7
        } else if context.coins >= 3 {
            6
        } else {
            5
        };

        let action_num: i32 = loop {
            let mut input = String::new();

            println!("\nSelect an action:");
            println!("1. Income: Collect 1 coin");
            println!("2. Foreign Aid: Collect 2 coins");
            println!("3. Tax: Collect 3 coins as Duke");
            println!("4. Steal: Take coins from another player as Captain");
            println!("5. Exchange: Replace cards as Ambassador");

            if context.coins >= 3 {
                println!("6. Assassinate: Pay 3 coins to assassinate another player as Assassin");
            }
            if context.coins >= 7 {
                println!("7. Coup: Pay 7 coins to launch a Coup on another player");
            }

            print!("> ");
            io::stdout().flush().unwrap();

            if io::stdin().read_line(&mut input).is_err() {
                println!("Failed to read input. Try again.");
                continue;
            }
            let trimmed = input.trim();

            if trimmed.eq_ignore_ascii_case("h") {
                print_role_reference();
                continue;
            }

            match trimmed.parse::<i32>() {
                Ok(n) if (1..=max_action).contains(&n) => break n,
                _ => {
                    println!(
                        "Invalid selection. Enter a number between 1 and {} (you have {} coins).",
                        max_action, context.coins
                    );
                    continue;
                }
            }
        };

        // map chosen number to Action variants that don't require targets
        match action_num {
            1 => return Action::Income,
            2 => return Action::ForeignAid,
            3 => return Action::Tax,
            5 => return Action::Swapping,
            _ => {} // fallthrough to actions that require a target (4,6,7)
        }

        // Must pick a target for actions 4, 6, 7.
        // Build list of candidates (exclude self if present)
        let candidates: Vec<_> = context
            .playing_bots
            .iter()
            .filter(|b| b.name != self.get_name())
            .collect();

        if candidates.is_empty() {
            // fallback: if there are no other players, treat as income
            println!("No other players to target — defaulting to Income.");
            return Action::Income;
        }

        let target_name: String = loop {
            println!("\nSelect a target:");
            for (i, bot) in candidates.iter().enumerate() {
                println!("{}: {}", i + 1, bot.name);
            }
            print!("> ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                println!("Failed to read input. Try again.");
                continue;
            }
            let trimmed = input.trim();
            if trimmed.is_empty() {
                println!("Please enter an index.");
                continue;
            }

            // parse as 1-based index into candidates
            match trimmed.parse::<usize>() {
                Ok(n) if n >= 1 && n <= candidates.len() => {
                    break candidates[n - 1].name.clone();
                }
                _ => {
                    println!(
                        "Invalid index. Enter a number between 1 and {}.",
                        candidates.len()
                    );
                    continue;
                }
            }
        };

        // return the targetting action
        match action_num {
            4 => Action::Stealing(target_name),
            6 => Action::Assassination(target_name),
            7 => Action::Coup(target_name),
            _ => {
                // shouldn't happen because we validated action_num earlier
                Action::Income
            }
        }
    }

	fn on_auto_coup(&self, context: &Context) -> String {
        loop {
            println!("Select a Coup target: ");
            let mut idx = 0;
            for bot in context.playing_bots.iter() {
                idx = idx + 1;
                println!("{}: {}", idx, bot.name);
            }
            print!("> ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");

            if let Ok(n) = input.parse::<usize>() {
                if n >= 1 && n <= context.playing_bots.len() {
                    return context.playing_bots[n - 1].name.clone();
                } else {
                    println!("Index out of range (1..={}).", context.playing_bots.len());
                    continue;
                }
            }
        }
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
            println!("Challenge?\n0: No\n1: Yes");
            print!("> ");

            io::stdout().flush().unwrap();
            
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
            println!("Counter?\n0: No\n1: Yes");
            print!("> ");

            io::stdout().flush().unwrap();
            
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
            println!("Challenge counter?\n0: No\n1: Yes");
            print!("> ");

            io::stdout().flush().unwrap();
            
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

     /// changed it 

	/// Swaps duplicate cards
    fn on_swapping_cards(
        &self,
        new_cards: [Card; 2],
        context: &Context,
    ) -> [Card; 2] {
    
        println!("\nYou are swapping cards.");
        println!("Choose 2 cards to keep.\n");
    
        // Combine old + new cards
        let mut options = Vec::new();
        options.extend_from_slice(&context.cards);
        options.extend_from_slice(&new_cards);
    
        // Display options
        for (i, card) in options.iter().enumerate() {
            println!("{}. {:?}", i + 1, card);
        }
    
        // Choose first card
        let first_choice = loop {
            let mut input = String::new();
            print!("Select first card to keep: ");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_line(&mut input).unwrap();
    
            if let Ok(num) = input.trim().parse::<usize>() {
                if num >= 1 && num <= options.len() {
                    break num - 1;
                }
            }
    
            println!("Invalid selection.");
        };
    
        // Choose second card
        let second_choice = loop {
            let mut input = String::new();
            print!("Select second card to keep: ");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_line(&mut input).unwrap();
    
            if let Ok(num) = input.trim().parse::<usize>() {
                if num >= 1 && num <= options.len() && (num - 1) != first_choice {
                    break num - 1;
                }
            }
    
            println!("Invalid selection.");
        };
    
        [options[first_choice], options[second_choice]]
    }



    fn on_card_loss(&self, context: &Context) -> Card {
        let cards = context.cards.clone();

        for (i, card) in cards.iter().enumerate() {
            println!("{}: {:#?}", i + 1, card);
        }

        loop {
            println!("Choose a card to lose:");
            print!("> ");
            io::stdout().flush().unwrap(); // Ensure prompt prints

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");

            if let Ok(num) = input.trim().parse::<usize>() {
                // Ensure valid 1-based input
                if num >= 1 && num <= cards.len() {
                    return cards[num - 1].clone(); // Adjust for 0-based index
                }
            }

            println!("Invalid selection, try again.");
        }
    }


}
