# Redrob Shape security model

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
- API keys reach the sidecar through the device flow or a pasted key, both kept in the OS keychain, or through `REDROB_API_KEY` in the process environment for development. Blender never holds one.
- Secrets are excluded from command lines, logs, `.blend` data, source control, and model context.

## Network behavior

The sidecar connects only to the configured HTTPS Redrob API base URL and, while connecting, the console's device endpoints under the same `/v1`. The bundled Blender integration respects offline state and remains fully usable when the agent cannot connect. A request timeout and response-size limit are applied, and cancellation terminates the in-flight request without mutating scene data.

## Data minimization

Scene context contains names, object types, transforms, dimensions, selected state, modifier summaries, and bounded material metadata. Mesh vertices, textures, image pixels, animation samples, file contents, absolute paths, and custom-property payloads are excluded by default.

## Failure behavior

- Sidecar absent: show an unavailable state; preserve all Blender functionality.
- Invalid response: show the parse or validation error; do not expose Apply.
- Stale scene: invalidate the plan and require a new request.
- Partial apply error: report the failing action and rely on Blender Undo for rollback.
- API key missing: show the Connect Redrob Console button without opening network connections.
- Process crash: close pipes, clear pending state, and allow an explicit restart.

## Device authorization

**Connect Redrob Console** runs the console's device flow (RFC 8628, product `shape`) in the sidecar. The device code, a bearer for the key until it is collected, never leaves the sidecar; Blender receives the user code, the approval page, and once connected only the key's last four characters and where it is kept. An approval page on any host other than the configured console is refused. The key goes to the OS credential store (Keychain on macOS, Credential Manager on Windows, the Secret Service on Linux) under `ai.redrob.shape` / `redrob`; without one it lasts for the session, and the panel says so. **Disconnect** removes it from this computer without restarting Blender. Revoking it everywhere is done in Redrob Console, where it is listed as "Redrob Shape (date)".
