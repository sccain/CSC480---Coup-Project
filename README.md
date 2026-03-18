Implementation of AI bot to play the game "Coup"
Created by: Dylan Gururajan, Shona Cain, Ashley Navos, Julianne Legados
Course: CSC 480, Instructed by Dr. Rodrigo Canaan

This project is forked from an existing repo: 
https://github.com/dominikwilkowski/coup

This pre-existing repo allows for the development of a bot to play Coup, as well 
as base functionality for playing the game.

The pre-existing repo comes with a few built in bots:

-- Honest Bot: never bluffs, will challenge if the opponent can not possibly 
               have the card they are bluffing

-- Random Bot: acts, blocks, and challenges randomly

-- Static Bot: take one coin every turn until it is able to coup an opponent

We developed a few bots:

-- Everything Bot: parameterized bot for bluffing

-- Duel Bot: uses memory and computation to determine when to bluff and challenge

-- MCTS Bot: the main focus of our project, building off of the Duel Bot. Uses a 
            Monte Carlo tree search style method to determine what moves to make,
            and uses memory to make strong challenges and bluffs against 
            opponents
 USAGE:

You will need to be able to run Rust programs via rustc and cargo

Install rustup (includes rustc and cargo) like this:
curl https://sh.rustup.rs -sSf | sh

To run a game, do this:
"cargo run"

Feel free to view the main.rs file to change the players in the game, or run
many games with aggregated results
This is important if you want to simulate some large number of games as we have
done in our report. Go to main.rs and instructions are there.

See other file "How_This_Works.md" for thorough breakdown of Coup and this repo.
