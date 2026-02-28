# Registry Format — Tmux Plugin Panel

This document describes the JSON schema for plugin registries used by **tppanel**.

## Overview

tppanel ships with a built-in registry (`registry.json`) compiled into the binary. You can add **external registries** — local JSON files or remote URLs — to extend the available plugins and themes.

## Registry JSON Schema

A registry file must be valid JSON with this top-level structure:

```json
{
  "version": 2,
  "updated": "2025-01-15",
  "plugins": [
    { ... },
    { ... }
  ]
}
```

### Top-Level Fields

| Field     | Type     | Required | Description                              |
|-----------|----------|----------|------------------------------------------|
| `version` | integer  | Yes      | Schema version (current: `2`)            |
| `updated` | string   | No       | ISO 8601 date of last update             |
| `plugins` | array    | Yes      | Array of plugin entries (see below)      |

### Plugin Entry

Each entry in the `plugins` array:

```json
{
  "repo": "tmux-plugins/tmux-sensible",
  "name": "Sensible",
  "description": "A set of sensible tmux defaults everyone can agree on",
  "category": "essential",
  "stars": 5500,
  "compat": ["tmux", "psmux"]
}
```

| Field         | Type     | Required | Description                                    |
|---------------|----------|----------|------------------------------------------------|
| `repo`        | string   | Yes      | GitHub repo path in `owner/name` format        |
| `name`        | string   | Yes      | Human-readable plugin name                     |
| `description` | string   | Yes      | Short description of what the plugin does       |
| `category`    | string   | Yes      | One of the category values below               |
| `stars`       | integer  | No       | GitHub star count (for sorting/display)        |
| `compat`      | string[] | Yes      | Compatibility: `"tmux"`, `"psmux"`, or both    |

### Category Values

| Value        | Label       | Description                               |
|--------------|-------------|-------------------------------------------|
| `essential`  | Essential   | Must-have plugins (TPM, sensible, etc.)   |
| `theme`      | Theme       | Color themes and visual styles            |
| `session`    | Session     | Session save/restore/management           |
| `navigation` | Navigation  | Pane/window navigation helpers            |
| `statusbar`  | Status Bar  | Status bar enhancements                   |
| `clipboard`  | Clipboard   | Copy/paste and yank integration           |
| `utility`    | Utility     | Everything else                           |

### Compatibility Values

| Value   | Description                                      |
|---------|--------------------------------------------------|
| `tmux`  | Compatible with tmux (Linux/macOS)               |
| `psmux` | Compatible with PSMux (Windows PowerShell mux)   |

A plugin can be compatible with both — use `["tmux", "psmux"]`.

## External Registry Sources

Registry sources are configured in `~/.config/tppanel/registry_sources.json`:

```json
{
  "sources": [
    {
      "name": "Built-in Registry",
      "url": "",
      "source_type": "embedded",
      "enabled": true
    },
    {
      "name": "My Custom Plugins",
      "url": "/home/user/.config/tppanel/my-plugins.json",
      "source_type": "local",
      "enabled": true
    },
    {
      "name": "Community Registry",
      "url": "https://example.com/tppanel-registry.json",
      "source_type": "remote",
      "enabled": true
    }
  ]
}
```

### Source Types

| Type       | Description                                        |
|------------|----------------------------------------------------|
| `embedded` | Built into the binary (always present, cannot be removed) |
| `local`    | A JSON file on the local filesystem                 |
| `remote`   | A URL that returns registry JSON (fetched via HTTP) |

### Source Fields

| Field         | Type    | Required | Description                            |
|---------------|---------|----------|----------------------------------------|
| `name`        | string  | Yes      | Display name for the source            |
| `url`         | string  | Yes*     | File path (local) or URL (remote). Empty for embedded. |
| `source_type` | string  | Yes      | `"embedded"`, `"local"`, or `"remote"` |
| `enabled`     | boolean | No       | Whether to load this source (default: `true`) |

## Example: Creating a Custom Registry

1. Create a JSON file following the schema:

```json
{
  "version": 2,
  "updated": "2025-01-15",
  "plugins": [
    {
      "repo": "myorg/my-tmux-plugin",
      "name": "My Plugin",
      "description": "Custom plugin for my team",
      "category": "utility",
      "stars": 0,
      "compat": ["tmux"]
    }
  ]
}
```

2. Add it as a source in `~/.config/tppanel/registry_sources.json`:

```json
{
  "sources": [
    {
      "name": "Built-in Registry",
      "url": "",
      "source_type": "embedded",
      "enabled": true
    },
    {
      "name": "Team Plugins",
      "url": "/path/to/team-registry.json",
      "source_type": "local",
      "enabled": true
    }
  ]
}
```

3. Restart tppanel — your plugins will appear in the Browse tab.

## Validation

tppanel validates registries on load. Common issues:
- Missing `version` field
- `repo` not in `owner/name` format
- Missing `compat` array
- Invalid JSON syntax

The built-in registry is always loaded first. External registries are merged — if a plugin `repo` already exists, the built-in version takes priority.

## Plugin Sources

### tmux Plugins

Most tmux plugins live under the [tmux-plugins](https://github.com/tmux-plugins) GitHub organization. The `repo` field uses `tmux-plugins/plugin-name` format.

### PSMux Plugins

PSMux plugins and themes are maintained in the [marlocarlo/psmux-plugins](https://github.com/marlocarlo/psmux-plugins) monorepo. Each subdirectory is a plugin, so the `repo` field uses `psmux-plugins/plugin-name` format.

Both types are supported in the same registry and can coexist.
