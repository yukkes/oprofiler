package com.openprofiler.agent;

import com.openprofiler.protocol.OProfilerProtocol;
import java.io.*;
import java.lang.management.ManagementFactory;
import java.lang.management.MemoryUsage;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public class TcpServer {
  private static final Pattern NATIVE_HOT_SPOT_PATTERN =
      Pattern.compile(
          "\\{\"id\":(\\d+),\"self_duration_nano\":(\\d+),\"total_duration_nano\":(\\d+),\"invocations\":(\\d+)\\}");

  private static class NativeJdbcHotSpot {
    final int id;
    final long selfDurationNano;
    final long totalDurationNano;
    final long invocations;

    NativeJdbcHotSpot(int id, long selfDurationNano, long totalDurationNano, long invocations) {
      this.id = id;
      this.selfDurationNano = selfDurationNano;
      this.totalDurationNano = totalDurationNano;
      this.invocations = invocations;
    }
  }

  private static class NativeAllocationHotSpot {
    final int id;
    final int parentId;
    final String className;
    final String methodName;
    final String methodDescriptor;
    final String allocatedType;
    final long allocatedSize;
    final long instanceCount;

    NativeAllocationHotSpot(
        int id,
        int parentId,
        String className,
        String methodName,
        String methodDescriptor,
        String allocatedType,
        long allocatedSize,
        long instanceCount) {
      this.id = id;
      this.parentId = parentId;
      this.className = className;
      this.methodName = methodName;
      this.methodDescriptor = methodDescriptor;
      this.allocatedType = allocatedType;
      this.allocatedSize = allocatedSize;
      this.instanceCount = instanceCount;
    }
  }

  private final int port;
  private final AtomicBoolean running = new AtomicBoolean(false);
  private ServerSocket serverSocket;
  private ExecutorService executor;

  public TcpServer(int port) {
    this.port = port;
  }

  public void start() {
    if (running.compareAndSet(false, true)) {
      executor = Executors.newCachedThreadPool();
      Thread serverThread = new Thread(this::runServer);
      serverThread.setName("OProfiler-TcpServer");
      serverThread.setDaemon(true);
      serverThread.start();
    }
  }

  public void stop() {
    running.set(false);
    if (serverSocket != null) {
      try {
        serverSocket.close();
      } catch (IOException ignored) {
      }
    }
    if (executor != null) {
      executor.shutdownNow();
    }
  }

  private void runServer() {
    try {
      serverSocket = new ServerSocket(port);
      System.out.println("[java-agent] TCP server listening on port " + port);
      while (running.get()) {
        try {
          Socket client = serverSocket.accept();
          executor.submit(() -> handleClient(client));
        } catch (IOException e) {
          if (running.get()) {
            System.err.println("[java-agent] Accept error: " + e.getMessage());
          }
        }
      }
    } catch (IOException e) {
      System.err.println("[java-agent] TCP server error: " + e.getMessage());
    }
  }

  private void handleClient(Socket client) {
    try (InputStream in = client.getInputStream();
        OutputStream out = client.getOutputStream()) {
      while (running.get() && !client.isClosed()) {
        OProfilerProtocol.Command cmd = readMessage(in, OProfilerProtocol.Command.parser());
        if (cmd == null) break;
        OProfilerProtocol.ProfilingData response = handleCommand(cmd);
        writeMessage(out, response);
      }
    } catch (IOException e) {
      if (running.get()) {
        System.err.println("[java-agent] Client handler error: " + e.getMessage());
      }
    } catch (Throwable t) {
      if (running.get()) {
        System.err.println("[java-agent] Client handler fatal error: " + t);
        t.printStackTrace(System.err);
      }
    } finally {
      try {
        client.close();
      } catch (IOException ignored) {
      }
    }
  }

  private OProfilerProtocol.ProfilingData handleCommand(OProfilerProtocol.Command cmd) {
    try {
      switch (cmd.getType()) {
        case START_CPU_RECORDING:
          JavaAgent.startRecording();
          return heartbeat();
        case STOP_CPU_RECORDING:
          JavaAgent.stopRecording();
          return heartbeat();
        case GET_CPU_DATA:
          return buildCpuData();
        case GET_MEMORY_DATA:
          return buildMemoryData();
        case SET_SAMPLING_INTERVAL:
          return handleSetSamplingInterval(cmd);
        default:
          return heartbeat();
      }
    } catch (Throwable t) {
      System.err.println("[java-agent] Command failed: " + cmd.getType() + ": " + t);
      t.printStackTrace(System.err);
      return heartbeat();
    }
  }

  private OProfilerProtocol.ProfilingData handleSetSamplingInterval(OProfilerProtocol.Command cmd) {
    if (cmd.hasSetSamplingInterval()) {
      OProfilerProtocol.SetSamplingIntervalCommand payload = cmd.getSetSamplingInterval();
      boolean enabled = payload.getIntervalMs() > 0;
      Profiler.setSamplingMode(enabled);
      System.out.println("[java-agent] Sampling mode set to: " + enabled);
    }
    return heartbeat();
  }

  private OProfilerProtocol.ProfilingData buildCpuData() {
    List<Profiler.MethodSnapshot> methodSnapshots = Profiler.getMethodSnapshots();
    List<Profiler.EdgeSnapshot> edgeSnapshots = Profiler.getEdgeSnapshots();
    Map<String, Profiler.MethodStats> stats = Profiler.getStats();
    long totalSelfDuration = 0;
    for (Profiler.MethodSnapshot method : methodSnapshots) {
      totalSelfDuration += method.selfDurationNano;
    }
    List<NativeJdbcHotSpot> nativeHotSpots = readNativeJdbcHotSpots();
    for (NativeJdbcHotSpot hotSpot : nativeHotSpots) {
      totalSelfDuration += hotSpot.selfDurationNano;
    }
    totalSelfDuration = Math.max(totalSelfDuration, 1);

    OProfilerProtocol.CpuData.Builder cpuBuilder = OProfilerProtocol.CpuData.newBuilder();
    OProfilerProtocol.MethodGraph.Builder graphBuilder = OProfilerProtocol.MethodGraph.newBuilder();
    for (Profiler.MethodSnapshot method : methodSnapshots) {
      double percent = (method.selfDurationNano * 100.0) / totalSelfDuration;
      OProfilerProtocol.HotSpot.Builder hs =
          OProfilerProtocol.HotSpot.newBuilder()
              .setClassName(method.className)
              .setMethodName(method.methodName)
              .setMethodDescriptor(method.methodDescriptor)
              .setSelfSamples(method.selfDurationNano)
              .setTotalSamples(method.totalDurationNano)
              .setSelfDurationNano(method.selfDurationNano)
              .setTotalDurationNano(method.totalDurationNano)
              .setPercent(percent)
              .setInvocations(method.invocations);
      cpuBuilder.addHotSpots(hs);
      graphBuilder.addNodes(
          OProfilerProtocol.MethodNode.newBuilder()
              .setId(method.id)
              .setClassName(method.className)
              .setMethodName(method.methodName)
              .setMethodDescriptor(method.methodDescriptor)
              .setExecutionCount(method.invocations)
              .setSelfDurationNano(method.selfDurationNano)
              .setTotalDurationNano(method.totalDurationNano));
    }
    for (Profiler.EdgeSnapshot edge : edgeSnapshots) {
      graphBuilder.addEdges(
          OProfilerProtocol.MethodEdge.newBuilder()
              .setFromNodeId(edge.fromMethodId)
              .setToNodeId(edge.toMethodId)
              .setCallCount(edge.callCount)
              .setTotalDurationNano(edge.totalDurationNano));
    }
    addNativeJdbcHotSpots(cpuBuilder, nativeHotSpots, totalSelfDuration);
    cpuBuilder.setMethodGraph(graphBuilder);

    return OProfilerProtocol.ProfilingData.newBuilder()
        .setType(OProfilerProtocol.ProfilingData.DataType.CPU_DATA)
        .setTimestampNano(System.nanoTime())
        .setCpuData(cpuBuilder)
        .build();
  }

  private OProfilerProtocol.ProfilingData buildMemoryData() {
    OProfilerProtocol.AllocationTree.Builder treeBuilder =
        OProfilerProtocol.AllocationTree.newBuilder();
    Profiler.beginAllocationRecordingSuppression();
    try {
      List<Profiler.AllocationSnapshot> javaAllocationHotSpots = Profiler.getAllocationSnapshots();
      if (!javaAllocationHotSpots.isEmpty()) {
        for (Profiler.AllocationSnapshot snapshot : javaAllocationHotSpots) {
          treeBuilder.addNodes(
              OProfilerProtocol.AllocationTreeNode.newBuilder()
                  .setId(snapshot.id)
                  .setParentId(snapshot.parentId)
                  .setClassName(snapshot.className)
                  .setMethodName(snapshot.methodName)
                  .setMethodDescriptor(snapshot.methodDescriptor)
                  .setAllocatedType(snapshot.allocatedType)
                  .setAllocatedSize(snapshot.allocatedSize)
                  .setInstanceCount(snapshot.instanceCount));
        }
      } else {
        List<NativeAllocationHotSpot> nativeAllocationHotSpots = readNativeAllocationHotSpots();
        for (NativeAllocationHotSpot hotSpot : nativeAllocationHotSpots) {
          treeBuilder.addNodes(
              OProfilerProtocol.AllocationTreeNode.newBuilder()
                  .setId(hotSpot.id)
                  .setParentId(hotSpot.parentId)
                  .setClassName(hotSpot.className)
                  .setMethodName(hotSpot.methodName)
                  .setMethodDescriptor(hotSpot.methodDescriptor)
                  .setAllocatedType(hotSpot.allocatedType)
                  .setAllocatedSize(hotSpot.allocatedSize)
                  .setInstanceCount(hotSpot.instanceCount));
        }
      }
    } finally {
      Profiler.endAllocationRecordingSuppression();
    }

    MemoryUsage heap = ManagementFactory.getMemoryMXBean().getHeapMemoryUsage();
    OProfilerProtocol.MemoryData memoryData =
        OProfilerProtocol.MemoryData.newBuilder()
            .setAllocationTree(treeBuilder)
            .setHeapUsedBytes(Math.max(0L, heap.getUsed()))
            .setHeapCommittedBytes(Math.max(0L, heap.getCommitted()))
            .build();

    return OProfilerProtocol.ProfilingData.newBuilder()
        .setType(OProfilerProtocol.ProfilingData.DataType.MEMORY_DATA)
        .setTimestampNano(System.nanoTime())
        .setMemoryData(memoryData)
        .build();
  }

  private List<NativeAllocationHotSpot> readNativeAllocationHotSpots() {
    List<NativeAllocationHotSpot> hotSpots = new ArrayList<>();
    if (!JavaAgent.isNativeTimingAvailable()) {
      return hotSpots;
    }
    String text;
    try {
      text = Agent.getNativeAllocationHotSpotsTsv();
    } catch (Throwable e) {
      return hotSpots;
    }
    if (text == null || text.isBlank()) {
      return hotSpots;
    }
    for (String line : text.split("\\R")) {
      if (line.isBlank()) {
        continue;
      }
      String[] parts = line.split("\\t", -1);
      if (parts.length < 8) {
        continue;
      }
      try {
        hotSpots.add(
            new NativeAllocationHotSpot(
                Integer.parseInt(parts[0]),
                Integer.parseInt(parts[1]),
                parts[2],
                parts[3],
                parts[4],
                parts[5],
                Long.parseLong(parts[6]),
                Long.parseLong(parts[7])));
      } catch (NumberFormatException ignored) {
      }
    }
    return hotSpots;
  }

  private List<NativeJdbcHotSpot> readNativeJdbcHotSpots() {
    List<NativeJdbcHotSpot> hotSpots = new ArrayList<>();
    if (!JavaAgent.isNativeTimingAvailable() || !"native".equals(JavaAgent.getJdbcProbeMode())) {
      return hotSpots;
    }
    String json;
    try {
      json = Agent.getNativeJdbcHotSpotsJson();
    } catch (Throwable e) {
      return hotSpots;
    }

    Matcher matcher = NATIVE_HOT_SPOT_PATTERN.matcher(json);
    while (matcher.find()) {
      int id = Integer.parseInt(matcher.group(1));
      if (id < 0 || id >= JdbcProbes.COUNT) {
        continue;
      }
      long selfDurationNano = Long.parseLong(matcher.group(2));
      long totalDurationNano = Long.parseLong(matcher.group(3));
      long invocations = Long.parseLong(matcher.group(4));
      if (invocations == 0) {
        continue;
      }
      long overhead = JavaAgent.jdbcProbeOverheadNanos(id) * invocations;
      selfDurationNano = Math.max(0, selfDurationNano - overhead);
      totalDurationNano = Math.max(0, totalDurationNano - overhead);
      hotSpots.add(new NativeJdbcHotSpot(id, selfDurationNano, totalDurationNano, invocations));
    }
    return hotSpots;
  }

  private void addNativeJdbcHotSpots(
      OProfilerProtocol.CpuData.Builder cpuBuilder,
      List<NativeJdbcHotSpot> nativeHotSpots,
      long totalSelfDuration) {
    for (NativeJdbcHotSpot hotSpot : nativeHotSpots) {
      double percent = (hotSpot.selfDurationNano * 100.0) / totalSelfDuration;
      OProfilerProtocol.HotSpot.Builder hs =
          OProfilerProtocol.HotSpot.newBuilder()
              .setClassName(JdbcProbes.CLASS_NAMES[hotSpot.id])
              .setMethodName(JdbcProbes.METHOD_NAMES[hotSpot.id])
              .setMethodDescriptor(JdbcProbes.DESCRIPTORS[hotSpot.id])
              .setSelfSamples(hotSpot.selfDurationNano)
              .setTotalSamples(hotSpot.totalDurationNano)
              .setSelfDurationNano(hotSpot.selfDurationNano)
              .setTotalDurationNano(hotSpot.totalDurationNano)
              .setPercent(percent)
              .setInvocations(hotSpot.invocations);
      cpuBuilder.addHotSpots(hs);
    }
  }

  private void addSqlHotSpots(
      OProfilerProtocol.CpuData.Builder cpuBuilder,
      List<SqlHotSpots.Snapshot> sqlHotSpots,
      long totalSelfDuration) {
    for (SqlHotSpots.Snapshot hotSpot : sqlHotSpots) {
      double percent = (hotSpot.totalDurationNano * 100.0) / totalSelfDuration;
      OProfilerProtocol.HotSpot.Builder hs =
          OProfilerProtocol.HotSpot.newBuilder()
              .setClassName("SQL")
              .setMethodName(hotSpot.sql)
              .setMethodDescriptor("")
              .setSelfSamples(hotSpot.totalDurationNano)
              .setTotalSamples(hotSpot.totalDurationNano)
              .setSelfDurationNano(hotSpot.totalDurationNano)
              .setTotalDurationNano(hotSpot.totalDurationNano)
              .setPercent(percent)
              .setInvocations(hotSpot.invocations);
      cpuBuilder.addHotSpots(hs);
    }
  }

  private OProfilerProtocol.ProfilingData heartbeat() {
    return OProfilerProtocol.ProfilingData.newBuilder()
        .setType(OProfilerProtocol.ProfilingData.DataType.HEARTBEAT)
        .setTimestampNano(System.nanoTime())
        .setHeartbeat(
            OProfilerProtocol.HeartbeatData.newBuilder()
                .setUptimeNano(System.nanoTime())
                .setActiveRecordings(JavaAgent.isRecording() ? 1 : 0))
        .build();
  }

  private static <T extends com.google.protobuf.Message> T readMessage(
      InputStream in, com.google.protobuf.Parser<T> parser) throws IOException {
    byte[] lenBytes = new byte[4];
    int read = readFullyOrEof(in, lenBytes, 0, lenBytes.length);
    if (read < 0) return null;
    int len =
        ((lenBytes[0] & 0xFF) << 24)
            | ((lenBytes[1] & 0xFF) << 16)
            | ((lenBytes[2] & 0xFF) << 8)
            | (lenBytes[3] & 0xFF);
    if (len > 50_000_000) throw new IOException("Message too large: " + len);
    byte[] data = new byte[len];
    readFullyOrThrow(in, data, 0, data.length);
    return parser.parseFrom(data);
  }

  private static int readFullyOrEof(InputStream in, byte[] data, int offset, int length)
      throws IOException {
    int off = offset;
    while (off < offset + length) {
      int read = in.read(data, off, offset + length - off);
      if (read < 0) {
        return off == offset ? -1 : off - offset;
      }
      off += read;
    }
    return off - offset;
  }

  private static void readFullyOrThrow(InputStream in, byte[] data, int offset, int length)
      throws IOException {
    int read = readFullyOrEof(in, data, offset, length);
    if (read < length) {
      throw new IOException("EOF");
    }
  }

  private static void writeMessage(OutputStream out, com.google.protobuf.Message msg)
      throws IOException {
    byte[] data = msg.toByteArray();
    out.write((data.length >>> 24) & 0xFF);
    out.write((data.length >>> 16) & 0xFF);
    out.write((data.length >>> 8) & 0xFF);
    out.write(data.length & 0xFF);
    out.write(data);
    out.flush();
  }
}
