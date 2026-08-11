# cf-gears-settings-service-sdk

Public SDK for the `settings-service` gear.

## Setting keys

A setting's key is a GTS **instance** identifier:

```text
gts.cf.toolkit.settings.types.bool_flag.v1~gts.acme.toolkit.settings.network.enable_proxy.v1
└──────────── value type, ends `~` ───────┘└────────── instance id, no trailing `~` ────────┘
```

- The **left** half is a curated value type from the `gts.cf.toolkit.settings.types.*~`
  catalog. It is the only part registered in GTS, and it defines the value's shape.
- The **right** half is the setting's own instance id. The setting is an unregistered
  GTS instance; it lives in the Settings DB, not the Registry.

For an admin-authored setting the instance id is
`gts.<vendor>.toolkit.settings.<category>.<name>.v1`, where `<category>` is the slug of
the category the setting was created in. Because the category is embedded, renaming or
moving a category re-keys every setting beneath it.

Contract source: ADR `settings-declaration-key-gts-type`, Amendment 2 (2026-07-12).

## Status

Phase 1 of the gear-foundation feature: the `SettingKey` value object, the opaque
`SecretHandle`, and the `EffectiveSource` vocabulary. Reader and contribution traits,
the error taxonomy, and the reader degradation contract arrive in phase 2.
