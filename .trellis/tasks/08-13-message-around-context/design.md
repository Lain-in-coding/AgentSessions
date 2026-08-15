# Design — Message around context

## Typed resolution

Application asks `ContextGraphStore` for message session candidates. A supplied session selects only a matching candidate. Without it, zero is not found, one resolves, and several return ambiguity. The selected session graph provides authoritative placements and mainline order.

## Window primitive

Create one application helper that locates the anchor placement in the ordered mainline and slices `[anchor-around, anchor+around]` with bounds clamped. The anchor is always retained; results stay chronological. Shared stable message identity does not collapse distinct placements.

## Request/response

Add an application message request/response so MCP remains thin. Response includes anchor/message/session/placement identifiers, ordered messages, and existing truncation metadata. Apply max-items and byte accounting after order is fixed.

## MCP

Add `get_message` to the tool catalog and validation/dispatch. Business ambiguity/not-found errors use the existing tool-error envelope; malformed parameters use JSON-RPC invalid params.

## Rollback

Remove the application variant and MCP tool. Existing context assembly and relation storage are unchanged.
