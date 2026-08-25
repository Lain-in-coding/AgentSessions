# Design — Context summary levels and hints

## Boundary

Application owns the level enum, structural aggregation, fallback, budget accounting, and hint generation. CLI/MCP only validate inputs and serialize the shared response.

## Data flow

1. Parse optional `level`; default to `raw`.
2. Load the existing typed `SessionContextGraph` and select the current branch exactly once.
3. Build raw messages first, then derive talks or a session overview from that ordered typed result.
4. If a requested derived level is empty, try the next more detailed level.
5. Clamp the chosen payload plus level metadata and hint under the existing response budget.

## Compatibility

The omitted/raw branch retains the current fields and ordering. New metadata is additive. No payload compatibility aliases are used for summary authority.

## Hint policy

Hints are templates selected from actual response state. Session summaries suggest `get_session_context` at `talks`; talks suggest `raw`; raw may suggest `get_message` only when a real message/session identifier exists. Hints are bounded before response assembly.

## Rollback

Remove the optional level input and additive fields; the underlying raw context path remains unchanged.
