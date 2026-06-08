# Gear Orchestrator Gear

System gear for service discovery.

## Overview

The `cf-gears-ochestrator` crate implements the `gear_orchestrator` gear.

It:

- Registers `DirectoryClient` in `ClientHub` for in-process gears
- Exposes the `DirectoryService` gRPC service (via `grpc-hub`)
- Uses the runtime `GearManager` for instance tracking and service resolution

## License

Licensed under Apache-2.0.
