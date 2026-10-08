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

import bpy
from bpy.props import BoolProperty, EnumProperty, StringProperty
from bpy.types import AddonPreferences, Operator, Panel

from . import credentials, transport

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


def resolve_now():
    prefs = preferences()
    preferred = None
    if prefs.transport_choice != "AUTO":
        preferred = [_KIND_BY_ITEM[prefs.transport_choice]]
    return transport.resolve(
        _STORE,
        _health_check,
        subscription_enabled=prefs.subscription_enabled,
        preferred=preferred,
    )


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


class REDROB_PT_connection(Panel):
    bl_idname = "REDROB_PT_connection"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Redrob Shape"
    bl_label = "Connection"
    bl_options = {"DEFAULT_CLOSED"}

    def draw(self, _context):
        layout = self.layout
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
            header.label(text="Add a key in Preferences, or start redrob-code")

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
    REDROB_PT_connection,
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
