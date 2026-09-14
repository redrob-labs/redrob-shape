# Redrob Reblend security model

## Trust boundaries

- Blender scene data is untrusted input to the agent serializer.
- Model output is untrusted until parsed and validated against the local action schema.
- Sidecar stdout is untrusted protocol input to the Blender process.
- API errors and model text are display data, never executable code.

## Non-negotiable controls

- No arbitrary Python or shell tool.
- No direct `bpy` access outside Blender's main thread.
- No scene mutation before an explicit preview and approval.
- Every mutation runs in a single undoable Blender operator.
- Unknown protocol versions, events, fields, and action types fail closed.
- Numeric values must be finite and within tool-specific limits.
- Object references are resolved again immediately before execution.
- Plans are bounded by action count and response size.
- API keys are accepted only through process environment or a future OS-keychain integration.
- Secrets are excluded from command lines, logs, `.blend` data, source control, and model context.

## Network behavior

The sidecar connects only to the configured HTTPS Redrob API base URL. The bundled Blender integration respects offline state and remains fully usable when the agent cannot connect. A request timeout and response-size limit are applied, and cancellation terminates the in-flight request without mutating scene data.

## Data minimization

Scene context contains names, object types, transforms, dimensions, selected state, modifier summaries, and bounded material metadata. Mesh vertices, textures, image pixels, animation samples, file contents, absolute paths, and custom-property payloads are excluded by default.

## Failure behavior

- Sidecar absent: show an unavailable state; preserve all Blender functionality.
- Invalid response: show the parse or validation error; do not expose Apply.
- Stale scene: invalidate the plan and require a new request.
- Partial apply error: report the failing action and rely on Blender Undo for rollback.
- API key missing: show setup guidance without opening network connections.
- Process crash: close pipes, clear pending state, and allow an explicit restart.

## Future device authorization

Console's short-code authorization can replace environment-key setup once its public application API contract is available. The implementation must keep the secret in the sidecar and OS credential store and must support key revocation without restarting Blender.
