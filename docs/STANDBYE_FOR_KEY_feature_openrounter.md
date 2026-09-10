

Note a key for openrouter is avliabae but only use free things to test the configuration (avaialbe on th env and loade don the chell, never thy to read it though) OPENROUTER_API_KEY
## 6.5 — OpenRouter live testing

### User stories
- As a **Dev**, I want a real (non-mock) provider test I can run for free, so that I can
  validate the OpenRouter path end-to-end without spending money like the Bedrock tests do.
- As a **Dev**, I want that live test off by default, so that `cargo test --workspace` and
  CI stay hermetic and offline.

### Approach
Provider code already exists (`openrouter.rs`, `model_catalog.rs`, `streaming.rs`) — this
is a *test* gap. Gate a live test behind a `live_openrouter` Cargo feature, mirroring the
existing `live_bedrock` convention in `tests/Cargo.toml`. Free tier (July 2026): 27+
`:free` models, **20 req/min** cap, 50/day (or 1,000/day after a one-time $10 credit). Pin
whichever `:free` id is live when the test is written; the roster rotates.

### Suggested implementation priority
1. One gated smoke test (connect, single non-streaming turn, assert a response) first —
   proves the path works at all.

   (this will test if the probing machinery of listing models with prices is wroking etc etc)

2. Another test (2 prompts only): Only need the smoketest I believe and maybe one easy prompt like: "what is 1+1, answer only with the answer, I want only to see 2, and nothing more on the answer", and then check if I see 2. Just to see if frameork is working.  And then do another quesiton asling 1+2,, but checking if logs caching etc make sense.
    (this using the free models)
