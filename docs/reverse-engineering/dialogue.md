# Dialogue: choosing a response

How the game picks which `INFO` (response) a topic (`DIAL`) gives. Condition evaluation
itself is in [conditions.md](conditions.md).

## Data shape

A topic holds, per quest that adds responses to it, that quest's list of `INFO`s in their
stored order. Response flags (`INFO` `DATA`) that matter here: `0x02` *Random*, `0x04`
*Say Once*, `0x20` *Random End*. Topic types (`DIAL` `DATA`): 0 topic, 1 conversation,
2 combat, 3 persuasion, 4 detection, 5 service, 6 miscellaneous.

## A response is eligible when

1. it isn't deleted;
2. its quest is **running** (topics only consider quests whose running flag is set);
3. it isn't a *Say Once* response that has already been said;
4. its condition list passes. When the quest itself has conditions, the quest's list and the
   response's list are evaluated together as one list (see conditions.md: lists can be
   chained); otherwise the response's own list alone.

## Picking

```
randoms = []
for each quest in the topic (stored order), if running:
    for each response of that quest (stored order):
        if not eligible: continue
        if response is Random:
            randoms.append(response)
            if response is Random End: stop scanning
            continue
        # a plain response
        if randoms is empty: return response          # first match wins
        stop scanning                                  # randoms collected before it win
if randoms not empty: return a uniformly random member of randoms
return nothing
```

So a run of *Random* responses forms a pool that ends at the first eligible plain response
(which is then *not* used) or at a *Random End* response (which is included).

**Disposition fallback.** While scanning, the picker remembers the last response whose
conditions failed only on a `GetDisposition >` or `>=` check. If nothing is eligible, the
picker returns that response together with a "disposition too low" marker, which is how the
game can show a topic but answer with a refusal.

**Retry for some topic types.** When nothing is eligible for a topic of type combat,
persuasion, detection or miscellaneous, and a particular player state is set, the picker
clears that state, tries again, and restores it (the state is not identified yet).

## Evidence

| Fact | Source | Confidence |
|------|--------|------------|
| Eligibility checks and chaining with the quest's conditions | Executable: the response eligibility test | confirmed (which list goes first: likely quest first) |
| Scan order, Random / Random End pooling, uniform pick | Executable: the response picker | confirmed |
| Disposition fallback | Executable: picker plus the condition list evaluator | confirmed |
| Retry for topic types 2, 3, 4, 6 | Executable: picker | confirmed (the player state: unknown) |

## Open

1. Quest order within a topic: stored (load) order, or sorted by quest priority at load.
2. The "choice" mode of the picker (used when building the topic list or following a
   response's choices) filters by previous responses; to document with the dialogue menu.
3. Which topics are listed in the menu (`AddTopic`, the player's known topics, greetings,
   *Goodbye*, *Run Immediately*, *Info Refusal*, rumours).
