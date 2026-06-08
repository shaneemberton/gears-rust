# Nodes Registry Gear

Node inventory and node system information for Gears

## Overview

The `cf-gears-nodes-registry` crate implements the `nodes_registry` gear.

The gear manages node information (host/VM/container) and provides REST endpoints to:

- List nodes
- Get node by ID
- Get node sysinfo (`/nodes/{id}/sysinfo`)
- Get node syscap (`/nodes/{id}/syscap`)

## Configuration

```yaml
gears:
  nodes_registry:
    config:
      enabled: true
```

## License

Licensed under Apache-2.0.
