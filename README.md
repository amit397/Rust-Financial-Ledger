# LedgerGuard
A type-safe Rust ledger that catches every AI agent mistake

## Motivation
Originally I was attempting to create an offline tool to help track expenses but that problem proved to be far more difficult than I thought, particularly because of the price behind financial connections (thank you Plaid), so I decided to pivot to something that could test my systems engineering skills instead.

After much brainstorming (primarily with LLMs) I came up with this project that tries to solve a few unique problems that may come to this space with some popular technologies. Most of that information is in its own md file (PROJECT_PLAN.md), but I'll include a brief (human) description of the problem and goals.

## Problem Statement
In the financial space mistakes are expensive (often worth billions) but there is also a desire to implement the newest technologies to save money (often worth billions). Of those technologies AI agents seem to be the buzz but they make a lot of mistakes, so I wanted to create a project that can help safeguard the implementation of AI agents on something that needs to essentially be perfect (Financial Ledger).

Again just as a disclaimer, the point of this is not to create a crazy good AI agent, moreso to make sure all mistakes that might be made by the agent is caught.

## Goals
* Create a simple AI agent that takes plain english transfers between accounts ("transfer $50 from Checking to Savings") and adds them to a ledger
* Build a fast ledger that can process previous transactions to enforce rules
* The ledger catches all of the AI agents mistakes (unbalanced entries, insufficient funds, nonexistent accounts, overflow)
