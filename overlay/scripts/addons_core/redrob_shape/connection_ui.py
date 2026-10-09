# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later
"""Connection UI: which transport Redrob Shape uses, and why.

The panel's job is to make the choice legible. A user who cannot tell whether their prompt went to
loopback, to Redrob's gateway, to their own billed API key, or to their subscription cannot make an
informed decision about any of it -- so every transport is listed with its state and its reason,
including the ones that are unavailable, because "why is it not using my key" is the question this
panel exists to answer.

Two things are deliberately absent:

  * No field ever shows a stored key. `credentials.describe()` reports its shape; the value is not
    readable back through the UI.
  * The subscription rows carry a standing warning. Access there depends on a vendor policy that
    moved three times in 2026 and that Anthropic has said it will revisit, so presenting it as
    settled would be misleading.
"""

# NOTE: deliberately NO `from __future__ import annotations` here.
#
# Blender registers add-on properties by evaluating the class annotations, so it needs the actual
# EnumProperty/BoolProperty objects. PEP 563 turns every annotation into a string, and registration
# then fails at add-on enable time -- a failure that only appears inside Blender, which is why the
# structural test in tests/test_connection_ui.py asserts the annotations are objects.

import time

import bpy
from bpy.props import BoolProperty, EnumProperty, StringProperty
from bpy.types import AddonPreferences, Operator, Panel

from . import credentials, layout_text, transport
from .i18n import iface_

_STORE = credentials.CredentialStore()

_TRANSPORT_ITEMS = (
    ("AUTO", "Automatic", "Use the first available transport, preferring local and gateway"),
    ("LOCAL", "redrob-code (local)", "The local engine on 127.0.0.1:4096"),
    ("GATEWAY", "Redrob gateway", "console.redrob.ai, using a Redrob key"),
    ("BYO", "Your own API key", "OpenAI or Anthropic, billed to your account"),
    ("SUBSCRIPTION", "Your Claude or ChatGPT plan", "Runs the vendor's own CLI, which authenticates itself"),
)

_KIND_BY_ITEM = {
    "LOCAL": transport.Kind.LOCAL,
    "GATEWAY": transport.Kind.GATEWAY,
    "BYO": transport.Kind.BYO,
    "SUBSCRIPTION": transport.Kind.SUBSCRIPTION,
}


def _health_check(url: str) -> bool:
    """Is the local engine up? Short timeout: this runs while drawing a panel."""
    import urllib.error
    import urllib.request

    try:
        with urllib.request.urlopen(f"{url}/global/health", timeout=0.4) as response:
            return 200 <= response.status < 300
    except Exception:
        return False


def preferences():
    return bpy.context.preferences.addons[__package__].preferences


def _session():
    return getattr(bpy.context.window_manager, "redrob_shape", None)


def resolve_now():
    prefs = preferences()
    preferred = None
    if prefs.transport_choice != "AUTO":
        preferred = [_KIND_BY_ITEM[prefs.transport_choice]]
    current = _session()
    return transport.resolve(
        _STORE,
        _health_check,
        subscription_enabled=prefs.subscription_enabled,
        preferred=preferred,
        gateway_connected=(current is not None and current.connection == 'CONNECTED'),
        supported=transport.AGENT_KINDS,
    )


def _agent():
    """The running agent, started on demand. None, with the reason reported, when it cannot run."""
    from . import runtime

    agent = runtime.runtime()
    if agent.running or agent.start():
        return agent
    return None


class REDROB_Preferences(AddonPreferences):
    bl_idname = __package__

    transport_choice: EnumProperty(
        name="Transport",
        description="Which way Redrob Shape reaches a model",
        items=_TRANSPORT_ITEMS,
        default="AUTO",
    )

    subscription_enabled: BoolProperty(
        name="Allow subscription access",
        description=(
            "Run the vendor's own CLI (Claude Agent SDK or Codex) so it authenticates with your "
            "subscription. Redrob Shape never sees or stores that credential. Vendor policy here has "
            "changed before and may change again"
        ),
        default=False,
    )

    # Deliberately not a StringProperty holding a key. An add-on preference is stored in
    # userpref.blend, an ordinary unprotected file that travels with a preferences backup; keys go
    # to the OS keyring through credentials.py instead. This field is write-only scratch space that
    # the operator clears immediately after storing.
    key_entry: StringProperty(
        name="Key",
        description="Paste a key here to store it in your operating system keyring",
        default="",
        subtype="PASSWORD",
        options={"SKIP_SAVE"},
    )

    key_target: EnumProperty(
        name="For",
        items=(
            ("redrob", "Redrob", "A Redrob gateway key"),
            ("openai", "OpenAI", "Your own OpenAI API key"),
            ("anthropic", "Anthropic", "Your own Anthropic API key"),
        ),
        default="redrob",
    )

    def draw(self, _context):
        layout = self.layout
        layout.prop(self, "transport_choice")
        layout.prop(self, "subscription_enabled")

        box = layout.box()
        box.label(text="Stored keys", icon="LOCKED")
        if not _STORE.durable:
            box.label(
                text="No OS keyring available: keys last for this session only",
                icon="ERROR",
            )
        for account, state in _STORE.status().items():
            row = box.row()
            if account == "redrob":
                # The agent keeps this one; ask it, and let the panel's Disconnect remove it.
                current = _session()
                state = _redrob_key_state(current) if current is not None else "unknown"
                row.label(text=f"{account}: {state}")
                continue
            row.label(text=f"{account}: {state}")
            remove = row.operator("redrob.forget_key", text="", icon="X")
            remove.account = account

        entry = layout.box()
        entry.label(text="Add a key")
        entry.prop(self, "key_target")
        entry.prop(self, "key_entry")
        entry.operator("redrob.store_key", icon="CHECKMARK")


class REDROB_OT_store_key(Operator):
    bl_idname = "redrob.store_key"
    bl_label = "Store key"
    bl_description = "Store the pasted key in your operating system keyring"

    def execute(self, _context):
        prefs = preferences()
        value = prefs.key_entry
        # Cleared before anything else can fail, so a rejected key does not linger in the field
        # where it would be written to userpref.blend on the next save.
        prefs.key_entry = ""
        if prefs.key_target == "redrob":
            # Same home as a device-flow key: the agent's keychain entry. Never the local store.
            agent = _agent()
            if agent is None:
                self.report({"ERROR"}, "The local agent is not running, so the key was not stored")
                return {"CANCELLED"}
            try:
                agent.store_key(value)
            except RuntimeError as error:
                self.report({"ERROR"}, credentials.redact(str(error)))
                return {"CANCELLED"}
            self.report({"INFO"}, "Sent the Redrob key to the system keychain")
            return {"FINISHED"}
        try:
            _STORE.set(prefs.key_target, value)
        except ValueError as error:
            self.report({"ERROR"}, credentials.redact(str(error)))
            return {"CANCELLED"}
        where = "keyring" if _STORE.durable else "this session only"
        self.report({"INFO"}, f"Stored the {prefs.key_target} key in {where}")
        return {"FINISHED"}


class REDROB_OT_forget_key(Operator):
    bl_idname = "redrob.forget_key"
    bl_label = "Forget key"
    bl_description = "Remove this key from your keyring. It is not revoked at the provider"

    account: StringProperty()

    def execute(self, _context):
        try:
            _STORE.delete(self.account)
        except ValueError as error:
            self.report({"ERROR"}, credentials.redact(str(error)))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Removed the {self.account} key from this machine")
        return {"FINISHED"}


_SOURCE_NOTES = {
    "keychain": ("Saved in your system keychain", "LOCKED"),
    "session": ("No system keychain here: connected until Blender closes", "INFO"),
    "environment": ("Using REDROB_API_KEY from the environment", "INFO"),
}


def online_access_locked(context=None):
    """True when Blender was started with --offline-mode: the preference cannot be switched on."""
    system = (context or bpy.context).preferences.system
    return system.is_property_readonly("use_online_access")


def draw_online_access(layout, context=None):
    """Blender ships with online access off, and Redrob Shape needs it. Say so first, with the
    switch right here, instead of letting the first Send fail."""
    box = layout.box()
    title = box.row()
    title.alert = True
    title.label(text="Online access is off", icon='INTERNET_OFFLINE')
    if online_access_locked(context):
        layout_text.wrapped_labels(
            box, iface_("Blender was started with --offline-mode. Restart it without that option.")
        )
        return
    layout_text.wrapped_labels(
        box, iface_("Redrob Shape reaches Redrob Console over the internet to make plans.")
    )
    row = box.row()
    row.scale_y = 1.4
    row.operator("redrob_shape.allow_online_access", icon='INTERNET')


def _redrob_key_state(current):
    if current.connection != 'CONNECTED':
        return "not connected"
    hint = f", ending {current.connection_hint}" if current.connection_hint else ""
    return f"connected ({current.connection_source or 'agent'}{hint})"


def _remaining(current):
    seconds = max(0, int(current.connect_expires_at - time.time()))
    return f"{seconds // 60}:{seconds % 60:02d}"


def draw_console_connection(layout, current):
    """The Redrob Console connection, in whichever state it is. Shared by both sidebar panels."""
    if not bpy.app.online_access:
        draw_online_access(layout)
        if current.connection != 'CONNECTED':
            return  # connecting needs the internet too: the switch above comes first
    box = layout.box()
    state = current.connection
    box.label(text="Redrob Console", icon='LINKED' if state == 'CONNECTED' else 'UNLINKED')

    if state == 'CONNECTED':
        box.label(
            text=iface_("Key ending {}").format(current.connection_hint) if current.connection_hint else iface_("Key stored"),
            icon='CHECKMARK',
            translate=False,
        )
        note = _SOURCE_NOTES.get(current.connection_source)
        if note:
            layout_text.wrapped_labels(box, iface_(note[0]), icon=note[1])
        if current.key_name:
            box.label(text=iface_("Listed in Console as {}").format(current.key_name), translate=False)
        row = box.row()
        # A key the environment supplies cannot be removed from here.
        row.enabled = current.connection_source != "environment"
        row.operator("redrob_shape.disconnect", icon='UNLINKED')
        return

    if state == 'WAITING':
        layout_text.wrapped_labels(box, iface_("Enter this code on the page that opened:"))
        code = box.row()
        code.alignment = 'CENTER'
        code.scale_y = 2.0
        code.label(text=current.user_code, translate=False)
        row = box.row(align=True)
        row.operator("redrob_shape.open_connect_page", icon='URL')
        row.operator("redrob_shape.copy_connect_code", text="", icon='COPYDOWN')
        box.label(
            text=iface_("Waiting for approval, expires in {}").format(_remaining(current)),
            icon='TIME',
            translate=False,
        )
        box.operator("redrob_shape.connect_cancel", icon='CANCEL')
        return

    if state == 'STARTING':
        box.label(text="Asking Redrob Console for a code", icon='TIME')
        return

    if state == 'UNKNOWN':
        box.label(text="Checking the connection", icon='TIME')
    elif current.connection_message:
        layout_text.wrapped_labels(
            box, current.connection_message, limit=4, icon='ERROR' if state == 'ERROR' else 'INFO'
        )
    else:
        layout_text.wrapped_labels(box, iface_("Connect to plan with your Redrob Console workspace"))
    connect = box.row()
    connect.scale_y = 1.4
    connect.operator("redrob_shape.connect", icon='LINKED')


class REDROB_OT_connect(Operator):
    bl_idname = "redrob_shape.connect"
    bl_label = "Connect Redrob Console"
    bl_description = (
        "Approve this computer in Redrob Console, in your browser. The key it issues is kept in "
        "your system keychain, never in Blender's files"
    )

    def execute(self, context):
        from . import runtime

        current = context.window_manager.redrob_shape
        if current.connection in {'STARTING', 'WAITING'}:
            return {"CANCELLED"}
        if not bpy.app.online_access:
            self.report({"WARNING"}, iface_("Allow online access first"))
            return {"CANCELLED"}
        agent = _agent()
        if agent is None:
            current.connection = 'ERROR'
            current.connection_message = runtime.runtime().start_error or iface_("The local agent could not start")
            self.report({"ERROR"}, current.connection_message)
            return {"CANCELLED"}
        current.connection = 'STARTING'
        current.connection_message = ""
        try:
            agent.connect_start()
        except RuntimeError as error:
            current.connection = 'ERROR'
            current.connection_message = str(error)
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        return {"FINISHED"}


class REDROB_OT_connect_cancel(Operator):
    bl_idname = "redrob_shape.connect_cancel"
    bl_label = "Cancel"
    bl_description = "Stop waiting for approval. The code stops working when it expires"

    def execute(self, context):
        from . import runtime

        current = context.window_manager.redrob_shape
        request_id = current.connect_request_id
        if request_id:
            try:
                runtime.runtime().cancel(request_id)
            except RuntimeError:
                pass
        runtime.clear_connect_code(current)
        current.connection = 'DISCONNECTED'
        current.connection_message = iface_("Connecting was cancelled")
        return {"FINISHED"}


class REDROB_OT_allow_online_access(Operator):
    bl_idname = "redrob_shape.allow_online_access"
    # Not "Allow Online Access": Blender's own Korean for that msgid would win over ours.
    bl_label = "Turn On Online Access"
    bl_description = (
        "Turn on Blender's own Allow Online Access preference, so Redrob Shape can reach Redrob "
        "Console. The scene stays on this computer; only what a plan needs is sent"
    )

    @classmethod
    def poll(cls, context):
        return not bpy.app.online_access and not online_access_locked(context)

    def invoke(self, context, event):
        return context.window_manager.invoke_confirm(self, event)

    def execute(self, context):
        preferences = context.preferences
        preferences.system.use_online_access = True
        # Kept for the next start too. With auto-save on, Blender saves preferences when it quits.
        if not preferences.use_preferences_save:
            bpy.ops.wm.save_userpref()
        if not bpy.app.online_access:
            self.report({"ERROR"}, iface_("Blender kept online access off"))
            return {"CANCELLED"}
        self.report({"INFO"}, iface_("Online access is on"))
        return {"FINISHED"}


class REDROB_OT_open_connect_page(Operator):
    bl_idname = "redrob_shape.open_connect_page"
    bl_label = "Open Approval Page"
    bl_description = "Open the Redrob Console page where you approve this computer"

    @classmethod
    def poll(cls, context):
        return bool(context.window_manager.redrob_shape.verification_uri_complete)

    def execute(self, context):
        bpy.ops.wm.url_open(url=context.window_manager.redrob_shape.verification_uri_complete)
        return {"FINISHED"}


class REDROB_OT_copy_connect_code(Operator):
    bl_idname = "redrob_shape.copy_connect_code"
    bl_label = "Copy Code"
    bl_description = "Copy the code, to type it on the approval page"

    def execute(self, context):
        context.window_manager.clipboard = context.window_manager.redrob_shape.user_code
        self.report({"INFO"}, iface_("Code copied"))
        return {"FINISHED"}


class REDROB_OT_disconnect(Operator):
    bl_idname = "redrob_shape.disconnect"
    bl_label = "Disconnect"
    bl_description = (
        "Remove the Redrob key from this computer's keychain. Revoke it in Redrob Console to stop "
        "it working anywhere else"
    )

    def invoke(self, context, event):
        return context.window_manager.invoke_confirm(self, event)

    def execute(self, _context):
        agent = _agent()
        if agent is None:
            self.report({"ERROR"}, "The local agent is not running")
            return {"CANCELLED"}
        try:
            agent.disconnect()
        except RuntimeError as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        return {"FINISHED"}


class REDROB_PT_connection(Panel):
    bl_idname = "REDROB_PT_connection"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Redrob Shape"
    bl_label = "Connection"
    bl_options = {"DEFAULT_CLOSED"}
    # Below the Redrob Shape panel: registration order would otherwise put it on top.
    bl_order = 1

    def draw(self, context):
        layout = self.layout
        draw_console_connection(layout, context.window_manager.redrob_shape)
        resolution = resolve_now()

        header = layout.box()
        if resolution.ok:
            header.label(text=resolution.summary(), icon="CHECKMARK")
            if resolution.chosen.volatile:
                header.label(
                    text="Vendor policy for subscription access can change",
                    icon="ERROR",
                )
        else:
            header.label(text="No transport available", icon="ERROR")
            header.label(text="Connect Redrob Console, or paste a Redrob key in Preferences")

        # Everything considered, not just the winner: "why is it not using my key" is the question
        # this panel exists to answer.
        details = layout.box()
        details.label(text="Available transports")
        for candidate in resolution.considered:
            row = details.row()
            row.enabled = candidate.available
            icon = "CHECKMARK" if candidate.available else "BLANK1"
            chosen = resolution.chosen is candidate
            label = f"{'▸ ' if chosen else ''}{candidate.label}"
            row.label(text=label, icon=icon)
            sub = details.row()
            sub.enabled = False
            sub.label(text=f"    {candidate.reason}")

        layout.operator("screen.userpref_show", text="Preferences", icon="PREFERENCES")


_CLASSES = (
    REDROB_Preferences,
    REDROB_OT_store_key,
    REDROB_OT_forget_key,
    REDROB_OT_connect,
    REDROB_OT_connect_cancel,
    REDROB_OT_allow_online_access,
    REDROB_OT_open_connect_page,
    REDROB_OT_copy_connect_code,
    REDROB_OT_disconnect,
    REDROB_PT_connection,
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
