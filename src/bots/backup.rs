use crate::{
    bot::{BotInterface, Context},
    Action, Card, History,
};

use std::cell::RefCell;

const N_CARDS: usize = 5;

#[derive(Default)]
struct Memory {
    seen_history_len: usize,
    opp_claims: [u32; N_CARDS],
}

thread_local! {
    static MEMORY: RefCell<Memory> = RefCell::new(Memory::default());
}

pub struct DuelBot;

fn card_idx(c: Card) -> usize {
    match c {
        Card::Duke => 0,
        Card::Assassin => 1,
        Card::Captain => 2,
        Card::Ambassador => 3,
        Card::Contessa => 4,
    }
}

impl DuelBot {
    // -------- Memory handling --------

    fn reset_if_new_game(context: &Context) {
        if context.history.is_empty() {
            MEMORY.with(|m| *m.borrow_mut() = Memory::default());
        }
    }

    fn bump_if_opponent(context: &Context, by: &str, card: Card) {
        let opp = context.playing_bots.iter().find(|b| b.name != context.name).unwrap();
        if by == opp.name {
            MEMORY.with(|m| {
                m.borrow_mut().opp_claims[card_idx(card)] += 1;
            });
        }
    }

    fn opp_claims(card: Card) -> u32 {
        MEMORY.with(|m| m.borrow().opp_claims[card_idx(card)])
    }

    fn update_from_history(context: &Context) {
        // Reset memory at the start of a new game
        if context.history.is_empty() {
            MEMORY.with(|m| *m.borrow_mut() = Memory::default());
            return;
        }

        MEMORY.with(|m| {
            let mut mem = m.borrow_mut();

            // Nothing new to process
            if mem.seen_history_len >= context.history.len() {
                return;
            }

            // In 1v1 there is exactly one opponent
            let opp_name = context
                .playing_bots
                .iter()
                .find(|b| b.name != context.name)
                .unwrap()
                .name
                .clone();

            for h in &context.history[mem.seen_history_len..] {
                match h {
                    History::ActionTax { by } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Duke)] += 1;
                    }

                    History::ActionAssassination { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Assassin)] += 1;
                    }

                    History::ActionStealing { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Captain)] += 1;
                    }

                    History::ActionSwapping { by } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Ambassador)] += 1;
                    }

                    History::CounterForeignAid { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Duke)] += 1;
                    }

                    History::CounterAssassination { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Contessa)] += 1;
                    }

                    History::CounterStealing { by, .. } if *by == opp_name => {
                        mem.opp_claims[card_idx(Card::Captain)] += 1;
                        mem.opp_claims[card_idx(Card::Ambassador)] += 1;
                    }

                    _ => {}
                }
            }

            // Mark all history as processed
            mem.seen_history_len = context.history.len();
        });
    }


    // -------- Card knowledge tables --------

    fn opponent<'a>(context: &'a Context) -> &'a crate::bot::OtherBot {
        context.playing_bots.iter().find(|b| b.name != context.name).unwrap()
    }

    fn visible_count(context: &Context, card: Card) -> usize {
        let hand = context.cards.iter().filter(|c| **c == card).count();
        let discard = context.discard_pile.iter().filter(|c| **c == card).count();
        hand + discard
    }

    fn remaining_copies(context: &Context, card: Card) -> i32 {
        (3 - Self::visible_count(context, card) as i32).max(0)
    }

    fn hidden_total(context: &Context) -> i32 {
        let visible = context.cards.len() as i32 + context.discard_pile.len() as i32;
        (15 - visible).max(0)
    }

    // -------- Probability model --------

    fn n_choose_k(n: i32, k: i32) -> f64 {
        if k < 0 || k > n { return 0.0; }
        let k = k.min(n - k);
        let mut num = 1.0;
        let mut den = 1.0;
        for i in 1..=k {
            num *= (n - (k - i)) as f64;
            den *= i as f64;
        }
        num / den
    }

    fn p_opponent_has(context: &Context, opp_cards: i32, card: Card) -> f64 {
        let n = Self::hidden_total(context);
        let k = Self::remaining_copies(context, card);

        if k == 0 || opp_cards <= 0 || n <= 0 {
            return 0.0;
        }

        let h = opp_cards.min(n);
        let base = 1.0 - (Self::n_choose_k(n - k, h) / Self::n_choose_k(n, h));

        let claims = Self::opp_claims(card) as f64;
        let mut odds = base / (1.0 - base + 1e-9);
        odds *= (0.35 * claims).exp();

        (odds / (1.0 + odds)).clamp(0.001, 0.999)
    }

    // -------- Decision helpers --------

    fn required_role_for_action(action: &Action) -> Option<Card> {
        match action {
            Action::Assassination(_) => Some(Card::Assassin),
            Action::Swapping => Some(Card::Ambassador),
            Action::Stealing(_) => Some(Card::Captain),
            Action::Tax => Some(Card::Duke),
            _ => None,
        }
    }

    fn should_challenge_action(context: &Context, _by: &str, action: &Action) -> bool {
        let Some(role) = Self::required_role_for_action(action) else { return false };

        if Self::remaining_copies(context, role) == 0 {
            return true;
        }

        let opp = Self::opponent(context);
        let p_has = Self::p_opponent_has(context, opp.cards as i32, role);

        let (v_win, v_lose) = match action {
            Action::Assassination(_) => (1.25, 1.0),
            Action::Tax => (1.10, 1.0),
            Action::Swapping => (0.80, 1.0),
            _ => (1.0, 1.0),
        };

        let life_factor = if context.cards.len() <= 1 { 1.6 } else { 1.0 };
        (1.0 - p_has) * v_win > p_has * v_lose * 1.10 * life_factor
    }

    fn bluff_ok(context: &Context, role: Card) -> bool {
        if Self::remaining_copies(context, role) == 0 {
            return false;
        }

        let opp = Self::opponent(context);
        let mut score = 0.22;

        if context.cards.len() >= 2 { score += 0.08; }
        if context.coins < opp.coins { score += 0.06; }
        if Self::remaining_copies(context, role) == 1 { score -= 0.10; }

        score > 0.22 && ((context.history.len() + context.coins as usize) % 4 == 0)
    }
}

impl BotInterface for DuelBot {
    fn get_name(&self) -> String {
        "DuelBot".to_string()
    }

    fn on_turn(&self, context: &Context) -> Action {
        Self::update_from_history(context);
        let target = Self::opponent(context).name.clone();

        if context.coins >= 7 {
            return Action::Coup(target);
        }

        if context.coins >= 3 &&
           (context.cards.contains(&Card::Assassin) || Self::bluff_ok(context, Card::Assassin)) {
            return Action::Assassination(target);
        }

        if context.cards.contains(&Card::Duke) || Self::bluff_ok(context, Card::Duke) {
            return Action::Tax;
        }

        let opp = Self::opponent(context);
        if opp.coins >= 2 &&
           (context.cards.contains(&Card::Captain) || Self::bluff_ok(context, Card::Captain)) {
            return Action::Stealing(opp.name.clone());
        }

        if Self::remaining_copies(context, Card::Duke) >= 2 &&
           context.history.len() % 3 == 0 {
            return Action::ForeignAid;
        }

        Action::Income
    }

    fn on_auto_coup(&self, context: &Context) -> String {
        Self::update_from_history(context);
        Self::opponent(context).name.clone()
    }

    fn on_challenge_action_round(&self, action: &Action, by: String, context: &Context) -> bool {
        Self::update_from_history(context);
        by != context.name && Self::should_challenge_action(context, &by, action)
    }

    fn on_counter(&self, action: &Action, _by: String, context: &Context) -> bool {
        Self::update_from_history(context);

        match action {
            Action::Assassination(_) =>
                context.cards.contains(&Card::Contessa) || Self::bluff_ok(context, Card::Contessa),

            Action::ForeignAid =>
                context.cards.contains(&Card::Duke) || Self::bluff_ok(context, Card::Duke),

            Action::Stealing(_) =>
                context.cards.contains(&Card::Captain)
                || context.cards.contains(&Card::Ambassador)
                || Self::bluff_ok(context, Card::Captain)
                || Self::bluff_ok(context, Card::Ambassador),

            _ => false,
        }
    }

    fn on_challenge_counter_round(&self, action: &Action, _by: String, context: &Context) -> bool {
        Self::update_from_history(context);

        match action {
            Action::Assassination(_) =>
                Self::remaining_copies(context, Card::Contessa) == 0,

            Action::ForeignAid =>
                Self::remaining_copies(context, Card::Duke) == 0,

            Action::Stealing(_) =>
                Self::remaining_copies(context, Card::Captain) == 0
                && Self::remaining_copies(context, Card::Ambassador) == 0,

            _ => false,
        }
    }

    fn on_swapping_cards(&self, new_cards: [Card; 2], context: &Context) -> [Card; 2] {
        Self::update_from_history(context);

        fn rank(c: Card) -> u8 {
            match c {
                Card::Duke => 5,
                Card::Assassin => 4,
                Card::Contessa => 3,
                Card::Captain => 2,
                Card::Ambassador => 1,
            }
        }

        let mut pool = vec![context.cards[0], context.cards[1], new_cards[0], new_cards[1]];
        pool.sort_by_key(|c| std::cmp::Reverse(rank(*c)));

        let keep1 = pool[0];
        let keep2 = pool[1];

        let mut discarded = Vec::new();
        let mut kept = 0;

        for c in [context.cards[0], context.cards[1], new_cards[0], new_cards[1]] {
            if kept < 2 && (c == keep1 || c == keep2) {
                kept += 1;
            } else {
                discarded.push(c);
            }
        }

        [discarded[0], discarded[1]]
    }

    fn on_card_loss(&self, context: &Context) -> Card {
        Self::update_from_history(context);

        fn rank(c: Card) -> u8 {
            match c {
                Card::Duke => 5,
                Card::Contessa => 4,
                Card::Assassin => 3,
                Card::Captain => 2,
                Card::Ambassador => 1,
            }
        }

        *context.cards.iter().min_by_key(|c| rank(**c)).unwrap()
    }
}

