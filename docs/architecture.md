# Architecture

This document describes the current public TUI-only implementation. It is written for maintainers who need to change the profiler without changing the user-facing behavior.

## Components

```text
TUI client
  -> discovers local JVMs with jps
  -> attaches the Java agent with tools/AttachAgent
  -> talks to the agent over protobuf/TCP

Java agent
  -> registers an ASM ClassFileTransformer
  -> instruments selected application classes
  -> records method, JDBC, SQL, and allocation statistics
  -> serves snapshots through TcpServer

Native JVMTI helper
  -> provides lower-overhead timing where the VM and OS allow it
  -> records native JDBC/allocation helper data
```

## Runtime Flow

1. The TUI lists local JVMs.
2. Selecting a JVM starts a session and starts CPU recording automatically.
3. If the JVM does not already expose the OpenProfiler TCP port, the TUI attaches `java-agent-0.1.0.jar`.
4. The agent starts a local TCP server.
5. The TUI sends `START_CPU_RECORDING`, waits for the agent to finish enabling instrumentation, then fetches `GET_CPU_DATA`.
6. CPU, Databases, Memory, and Threads views render snapshots from the shared `crates/core` models.

## Protocol

The protocol uses protobuf messages framed by a 4-byte big-endian length prefix. `crates/core/src/protocol.rs` owns the client-side framing and conversion into UI models.

Implemented commands:

- `START_CPU_RECORDING`
- `STOP_CPU_RECORDING`
- `GET_CPU_DATA`
- `GET_MEMORY_DATA`
- `SET_SAMPLING_INTERVAL` as a compatibility toggle

Unsupported protocol commands should fail clearly or return heartbeat-compatible responses until implemented.

## Instrumentation Filtering

The Java agent supports target filtering to keep class retransformation fast:

- `includes=<prefix[,prefix...]>`
- `excludes=<prefix[,prefix...]>`

The TUI infers a default include prefix from the selected JVM main class when possible. This keeps recording startup practical for normal applications.

## Maintenance Rules

- Keep TUI state and rendering separate where possible.
- Keep JVM discovery, attach, TCP, and protocol conversion in `crates/core`.
- Keep source code comments in English.
- Do not commit `target/`, `dist/`, generated jar files, or temporary expanded dependency folders.
- When changing agent command handling, verify both the Java agent build and the Rust fake-agent integration tests.
- When changing packaging, verify the generated release folder contains the TUI binary, Java agent jar, native library, and attach helper.
