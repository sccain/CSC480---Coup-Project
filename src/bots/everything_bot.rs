use rand::Rng;

    // PUT IN MAIN WHEN USING EVERYTHING BOT
    // let bluff_bot = EverythingBot::new(
    //     "BluffBot",
    //     EverythingBotConfig {
    //         bluff_tax_chance: 0.6,
    //         bluff_assassinate_chance: 0.3,
    //         bluff_steal_chance: 0.2,
    //         bluff_counter_foreign_aid_chance: 0.4,
    //         bluff_counter_assassination_chance: 0.2,
    //         bluff_counter_steal_chance: 0.3,
    //         challenge_action_chance: 0.1,
    //         challenge_counter_chance: 0.1,
    //     }
    // );

use crate::{
	bot::{BotInterface, Context},
	Action, Card,
};

#[derive(Debug, Clone)]
pub struct EverythingBotConfig {
	/// Chance to bluff Duke and play Tax without Duke
	pub bluff_tax_chance: f64,
	/// Chance to bluff Assassin and play Assassination without Assassin
	pub bluff_assassinate_chance: f64,
	/// Chance to bluff Captain and play Stealing without Captain
	pub bluff_steal_chance: f64,

	/// Chance to bluff Duke as a counter to Foreign Aid
	pub bluff_counter_foreign_aid_chance: f64,
	/// Chance to bluff Contessa as a counter to Assassination
	pub bluff_counter_assassination_chance: f64,
	/// Chance to bluff Captain/Ambassador as a counter to Stealing
	pub bluff_counter_steal_chance: f64,

	/// Chance to challenge an action when not certain
	pub challenge_action_chance: f64,
	/// Chance to challenge a counter when not certain
	pub challenge_counter_chance: f64,
}

impl Default for EverythingBotConfig {
	fn default() -> Self {
		Self {
			bluff_tax_chance: 0.0,
			bluff_assassinate_chance: 0.0,
			bluff_steal_chance: 0.0,
			bluff_counter_foreign_aid_chance: 0.0,
			bluff_counter_assassination_chance: 0.0,
			bluff_counter_steal_chance: 0.0,
			challenge_action_chance: 0.0,
			challenge_counter_chance: 0.0,
		}
	}
}

pub struct EverythingBot {
	pub name: String,
	pub config: EverythingBotConfig,
}

impl EverythingBot {
	pub fn new(name: impl Into<String>, config: EverythingBotConfig) -> Self {
		Self {
			name: name.into(),
			config,
		}
	}

	fn roll(chance: f64) -> bool {
		debug_assert!((0.0..=1.0).contains(&chance));
		rand::thread_rng().gen_bool(chance.clamp(0.0, 1.0))
	}

	fn weakest_target_name(&self, context: &Context) -> String {
	    context
		.playing_bots
		.iter()
		.filter(|bot| bot.name != context.name)
		.min_by_key(|bot| bot.cards)
		.unwrap()
		.name
		.clone()
}

	fn visible_count(context: &Context, target_card: Card) -> usize {
		context
			.cards
			.iter()
			.chain(context.discard_pile.iter())
			.filter(|card| **card == target_card)
			.count()
	}
}

impl BotInterface for EverythingBot {
	fn get_name(&self) -> String {
		self.name.clone()
	}

	fn on_turn(&self, context: &Context) -> Action {
        let target = self.weakest_target_name(context);

        if context.cards.contains(&Card::Assassin) && context.coins >= 3 {
            return Action::Assassination(target);
        }

        if context.coins >= 3 && Self::roll(self.config.bluff_assassinate_chance) {
            return Action::Assassination(target);
        }

        if context.cards.contains(&Card::Captain) {
            return Action::Stealing(target);
        }

        if Self::roll(self.config.bluff_steal_chance) {
            return Action::Stealing(target);
        }

        if context.cards.contains(&Card::Duke) {
            return Action::Tax;
        }

        if Self::roll(self.config.bluff_tax_chance) {
            return Action::Tax;
        }

	    Action::Income
    }

	fn on_auto_coup(&self, context: &Context) -> String {
	    self.weakest_target_name(context)
    }

	fn on_challenge_action_round(
		&self,
		action: &Action,
		_by: String,
		context: &Context,
	) -> bool {
		let certain_challenge = match action {
			Action::Assassination(_) => Self::visible_count(context, Card::Assassin) == 3,
			Action::Swapping => Self::visible_count(context, Card::Ambassador) == 3,
			Action::Stealing(_) => Self::visible_count(context, Card::Captain) == 3,
			Action::Tax => Self::visible_count(context, Card::Duke) == 3,
			Action::Coup(_) | Action::ForeignAid | Action::Income => {
				unreachable!("Can't challenge Coup, Foreign Aid, or Income here")
			},
		};

		if certain_challenge {
			true
		} else {
			Self::roll(self.config.challenge_action_chance)
		}
	}

	fn on_counter(
		&self,
		action: &Action,
		_by: String,
		context: &Context,
	) -> bool {
		match action {
			Action::Assassination(_) => {
				context.cards.contains(&Card::Contessa)
					|| Self::roll(self.config.bluff_counter_assassination_chance)
			},
			Action::ForeignAid => {
				context.cards.contains(&Card::Duke)
					|| Self::roll(self.config.bluff_counter_foreign_aid_chance)
			},
			Action::Stealing(_) => {
				context.cards.contains(&Card::Captain)
					|| context.cards.contains(&Card::Ambassador)
					|| Self::roll(self.config.bluff_counter_steal_chance)
			},
			Action::Coup(_) | Action::Swapping | Action::Income | Action::Tax => {
				unreachable!("Can't counter this action")
			},
		}
	}

	fn on_challenge_counter_round(
		&self,
		action: &Action,
		_by: String,
		context: &Context,
	) -> bool {
		let certain_challenge = match action {
			Action::Assassination(_) => Self::visible_count(context, Card::Contessa) == 3,

			// Countering Foreign Aid requires Duke.
			Action::ForeignAid => Self::visible_count(context, Card::Duke) == 3,

			// A Steal counter can be Captain OR Ambassador.
			// So it's only certainly impossible if all 3 Captains and all 3 Ambassadors are visible.
			Action::Stealing(_) => {
				Self::visible_count(context, Card::Captain) == 3
					&& Self::visible_count(context, Card::Ambassador) == 3
			},

			Action::Coup(_) | Action::Income | Action::Swapping | Action::Tax => {
				unreachable!("Can't challenge counter here")
			},
		};

		if certain_challenge {
			true
		} else {
			Self::roll(self.config.challenge_counter_chance)
		}
	}

	fn on_swapping_cards(
		&self,
		new_cards: [Card; 2],
		context: &Context,
	) -> [Card; 2] {
		// Simple strategy:
		// - Prefer keeping non-duplicate cards
		// - Prefer stronger influence cards roughly in this order:
		//   Duke > Assassin > Captain > Contessa > Ambassador

		let mut all_cards = vec![
			context.cards[0],
			context.cards[1],
			new_cards[0],
			new_cards[1],
		];

		fn score(card: Card) -> i32 {
			match card {
				Card::Duke => 5,
				Card::Assassin => 4,
				Card::Captain => 3,
				Card::Contessa => 2,
				Card::Ambassador => 1,
			}
		}

		all_cards.sort_by_key(|card| -score(*card));

		let keep1 = all_cards[0];
		let keep2 = all_cards
			.iter()
			.copied()
			.find(|card| *card != keep1)
			.unwrap_or(all_cards[1]);

		// Return the two cards to discard
		let mut remaining = all_cards.clone();

		let pos1 = remaining.iter().position(|c| *c == keep1).unwrap();
		remaining.remove(pos1);

		let pos2 = remaining.iter().position(|c| *c == keep2).unwrap();
		remaining.remove(pos2);

		[remaining[0], remaining[1]]
	}

	fn on_card_loss(&self, context: &Context) -> Card {
		// Lose the weaker card first.
		fn score(card: Card) -> i32 {
			match card {
				Card::Duke => 5,
				Card::Assassin => 4,
				Card::Captain => 3,
				Card::Contessa => 2,
				Card::Ambassador => 1,
			}
		}

		*context
			.cards
			.iter()
			.min_by_key(|card| score(**card))
			.unwrap()
	}
}