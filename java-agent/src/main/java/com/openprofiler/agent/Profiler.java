package com.openprofiler.agent;

import java.lang.management.ManagementFactory;
import java.lang.management.ThreadMXBean;
import java.lang.ref.WeakReference;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;

public class Profiler {
  private static volatile java.lang.instrument.Instrumentation instrumentation;

  public static class MethodStats {
    public final AtomicLong selfDurationNano = new AtomicLong(0);
    public final AtomicLong totalDurationNano = new AtomicLong(0);
    public final AtomicLong invocationCount = new AtomicLong(0);
  }

  public static class MethodSnapshot {
    public final int id;
    public final String className;
    public final String methodName;
    public final String methodDescriptor;
    public final long selfDurationNano;
    public final long totalDurationNano;
    public final long invocations;

    MethodSnapshot(int id, MethodMeta meta, MethodStats stats) {
      this.id = id;
      this.className = meta.className;
      this.methodName = meta.methodName;
      this.methodDescriptor = meta.methodDescriptor;
      this.selfDurationNano = stats.selfDurationNano.get();
      this.totalDurationNano = stats.totalDurationNano.get();
      this.invocations = stats.invocationCount.get();
    }
  }

  public static class EdgeSnapshot {
    public final int fromMethodId;
    public final int toMethodId;
    public final long callCount;
    public final long totalDurationNano;

    EdgeSnapshot(int fromMethodId, int toMethodId, EdgeStats stats) {
      this.fromMethodId = fromMethodId;
      this.toMethodId = toMethodId;
      this.callCount = stats.callCount.get();
      this.totalDurationNano = stats.totalDurationNano.get();
    }
  }

  public static class AllocationSnapshot {
    public final int id;
    public final int parentId;
    public final String className;
    public final String methodName;
    public final String methodDescriptor;
    public final String allocatedType;
    public final long allocatedSize;
    public final long instanceCount;

    AllocationSnapshot(
        int id,
        int parentId,
        MethodMeta meta,
        String allocatedType,
        long allocatedSize,
        long instanceCount) {
      this.id = id;
      this.parentId = parentId;
      this.className = meta == null ? "" : meta.className;
      this.methodName = meta == null ? "" : meta.methodName;
      this.methodDescriptor = meta == null ? "" : meta.methodDescriptor;
      this.allocatedType = allocatedType;
      this.allocatedSize = allocatedSize;
      this.instanceCount = instanceCount;
    }
  }

  private static class EdgeStats {
    final AtomicLong callCount = new AtomicLong(0);
    final AtomicLong totalDurationNano = new AtomicLong(0);
  }

  private static class AllocationStats {
    final int methodId;
    final String parentPath;
    final String allocatedType;
    final AtomicLong allocatedSize = new AtomicLong(0);
    final AtomicLong instanceCount = new AtomicLong(0);
    final ConcurrentLinkedQueue<AllocationSample> liveSamples = new ConcurrentLinkedQueue<>();

    AllocationStats(int methodId, String parentPath, String allocatedType) {
      this.methodId = methodId;
      this.parentPath = parentPath;
      this.allocatedType = allocatedType;
    }
  }

  private static class AllocationSample {
    final WeakReference<Object> reference;
    final long size;

    AllocationSample(Object object, long size) {
      this.reference = new WeakReference<>(object);
      this.size = size;
    }
  }

  private static class LiveAllocationTotals {
    final long size;
    final long count;

    LiveAllocationTotals(long size, long count) {
      this.size = size;
      this.count = count;
    }
  }

  private static class StackFrame {
    final int methodId;
    final boolean allocationBoundary;

    StackFrame(int methodId, boolean allocationBoundary) {
      this.methodId = methodId;
      this.allocationBoundary = allocationBoundary;
    }
  }

  private static final class MethodStack {
    private static final byte SUBTRACT_CHILDREN = 1;
    private static final byte ALLOCATION_BOUNDARY = 2;
    private static final byte SAMPLED = 4;
    private static final byte TRANSPARENT_TO_PARENT = 8;

    private int[] methodIds = new int[128];
    private long[] startNanos = new long[128];
    private long[] childDurationNanos = new long[128];
    private long[] overrideDurationNanos = new long[128];
    private byte[] flags = new byte[128];
    private int depth = 0;

    void push(
        int methodId,
        long startNano,
        boolean subtractChildren,
        boolean allocationBoundary,
        boolean sampled) {
      ensureDepth(depth + 1);
      methodIds[depth] = methodId;
      startNanos[depth] = startNano;
      childDurationNanos[depth] = 0;
      overrideDurationNanos[depth] = 0;
      byte frameFlags = 0;
      if (subtractChildren) {
        frameFlags |= SUBTRACT_CHILDREN;
      }
      if (allocationBoundary) {
        frameFlags |= ALLOCATION_BOUNDARY;
      }
      if (sampled) {
        frameFlags |= SAMPLED;
      }
      flags[depth] = frameFlags;
      depth++;
    }

    int popMethodId() {
      if (depth == 0) {
        return Integer.MIN_VALUE;
      }
      depth--;
      return methodIds[depth];
    }

    int findFromTop(int methodId) {
      for (int i = depth - 1; i >= 0; i--) {
        if (methodIds[i] == methodId) {
          return i;
        }
      }
      return -1;
    }

    void truncate(int newDepth) {
      if (newDepth >= 0 && newDepth <= depth) {
        depth = newDepth;
      }
    }

    boolean isEmpty() {
      return depth == 0;
    }

    int topIndex() {
      return depth - 1;
    }

    int methodIdAt(int index) {
      return methodIds[index];
    }

    long startNanoAt(int index) {
      return startNanos[index];
    }

    long childDurationAt(int index) {
      return childDurationNanos[index];
    }

    long overrideDurationAt(int index) {
      return overrideDurationNanos[index];
    }

    boolean subtractChildrenAt(int index) {
      return (flags[index] & SUBTRACT_CHILDREN) != 0;
    }

    boolean sampledAt(int index) {
      return (flags[index] & SAMPLED) != 0;
    }

    boolean transparentToParentAt(int index) {
      return (flags[index] & TRANSPARENT_TO_PARENT) != 0;
    }

    void addChildDuration(int index, long duration) {
      childDurationNanos[index] += duration;
    }

    void addOverrideDurationToAll(long duration) {
      for (int i = 0; i < depth; i++) {
        overrideDurationNanos[i] += duration;
      }
    }

    void markTopTransparentToParent() {
      if (depth > 0) {
        flags[depth - 1] |= TRANSPARENT_TO_PARENT;
      }
    }

    StackFrame[] framesFromTop() {
      StackFrame[] frames = new StackFrame[depth];
      for (int i = 0; i < depth; i++) {
        int source = depth - 1 - i;
        frames[i] = new StackFrame(methodIds[source], (flags[source] & ALLOCATION_BOUNDARY) != 0);
      }
      return frames;
    }

    private void ensureDepth(int required) {
      if (required <= methodIds.length) {
        return;
      }
      int newLength = methodIds.length;
      while (newLength < required) {
        newLength *= 2;
      }
      int[] newMethodIds = new int[newLength];
      long[] newStartNanos = new long[newLength];
      long[] newChildDurationNanos = new long[newLength];
      long[] newOverrideDurationNanos = new long[newLength];
      byte[] newFlags = new byte[newLength];
      System.arraycopy(methodIds, 0, newMethodIds, 0, methodIds.length);
      System.arraycopy(startNanos, 0, newStartNanos, 0, startNanos.length);
      System.arraycopy(childDurationNanos, 0, newChildDurationNanos, 0, childDurationNanos.length);
      System.arraycopy(
          overrideDurationNanos, 0, newOverrideDurationNanos, 0, overrideDurationNanos.length);
      System.arraycopy(flags, 0, newFlags, 0, flags.length);
      methodIds = newMethodIds;
      startNanos = newStartNanos;
      childDurationNanos = newChildDurationNanos;
      overrideDurationNanos = newOverrideDurationNanos;
      flags = newFlags;
    }
  }

  private static final class LocalCounters {
    long[] selfDuration = new long[4096];
    long[] totalDuration = new long[4096];
    long[] invocationCount = new long[4096];
    long sampleCounter = 0;
    long rateCount = 0;
    long rateWindowStartedAtMs = 0;
    int unsampledDepth = 0;
    int sampledDepth = 0;

    void ensureCapacity(int required) {
      if (required <= invocationCount.length) {
        return;
      }
      int newLength = invocationCount.length;
      while (newLength < required) {
        newLength *= 2;
      }
      selfDuration = copyOf(selfDuration, newLength);
      totalDuration = copyOf(totalDuration, newLength);
      invocationCount = copyOf(invocationCount, newLength);
    }

    private static long[] copyOf(long[] source, int newLength) {
      long[] copy = new long[newLength];
      System.arraycopy(source, 0, copy, 0, source.length);
      return copy;
    }
  }

  private static class MethodMeta {
    final String className;
    final String methodName;
    final String methodDescriptor;
    final String key;

    MethodMeta(String className, String methodName, String methodDescriptor) {
      this.className = className.replace('/', '.');
      this.methodName = methodName;
      this.methodDescriptor = methodDescriptor;
      this.key = this.className + "." + methodName + methodDescriptor;
    }
  }

  private static final ConcurrentHashMap<String, MethodStats> stats = new ConcurrentHashMap<>();
  private static final ConcurrentHashMap<String, Integer> methodIdsByKey =
      new ConcurrentHashMap<>();
  private static final ConcurrentHashMap<Long, EdgeStats> edgeStats = new ConcurrentHashMap<>();
  private static final ConcurrentHashMap<String, AllocationStats> allocationStats =
      new ConcurrentHashMap<>();
  private static final AtomicInteger nextMethodId = new AtomicInteger();
  private static volatile MethodMeta[] methodMetas = new MethodMeta[4096];
  private static volatile MethodStats[] methodStats = new MethodStats[4096];
  private static final ThreadLocal<MethodStack> stack = ThreadLocal.withInitial(MethodStack::new);
  private static final ThreadLocal<Long> overrideStartedAtNano = new ThreadLocal<>();
  private static final ThreadLocal<Boolean> allocationRecorderActive =
      ThreadLocal.withInitial(() -> false);
  private static final MethodStats[] jdbcStats = new MethodStats[JdbcProbes.COUNT];
  private static final ThreadLocal<JdbcProbeStack> jdbcProbeStack =
      ThreadLocal.withInitial(JdbcProbeStack::new);
  private static final ThreadMXBean threadBean = ManagementFactory.getThreadMXBean();
  private static final boolean cpuTimeAvailable = initCpuTime();

  // Sampling mechanism: reduce nanoTime() calls when call rate is high
  private static final boolean DEFAULT_SAMPLING_MODE =
      Boolean.parseBoolean(System.getProperty("openprofiler.cpu.sampling.default", "true"));
  private static volatile boolean samplingMode = DEFAULT_SAMPLING_MODE;
  private static final int SAMPLING_INTERVAL =
      Integer.getInteger("openprofiler.cpu.sampling.interval", 100);
  private static final long CALL_RATE_THRESHOLD = 10_000;
  private static final long RATE_WINDOW_MS = 500;
  private static final long RATE_CHECK_MASK = 4095;

  // Optimized: Edge stats disabled by default to reduce ConcurrentHashMap overhead
  private static volatile boolean edgeTrackingEnabled = false;

  // Optimized: Thread-local accumulators to avoid AtomicLong CAS on hot path
  private static final ConcurrentLinkedQueue<LocalCounters> allLocalCounters =
      new ConcurrentLinkedQueue<>();
  private static final ThreadLocal<LocalCounters> localCounters =
      ThreadLocal.withInitial(
          () -> {
            LocalCounters counters = new LocalCounters();
            allLocalCounters.add(counters);
            return counters;
          });

  // Optimized: static final flags resolved at startup for JIT branch prediction
  private static final boolean useNativeCycleTime;
  private static final boolean useNativeCpuTime;

  static {
    boolean nativeCycle = false;
    boolean nativeCpu = false;
    try {
      if (JavaAgent.isNativeTimingAvailable()) {
        long test = Agent.currentThreadCpuCycleTimeNanos();
        if (test >= 0) {
          nativeCycle = true;
        } else {
          test = Agent.currentThreadCpuTimeNanos();
          if (test >= 0) {
            nativeCpu = true;
          }
        }
      }
    } catch (Throwable ignored) {
    }
    useNativeCycleTime = nativeCycle;
    useNativeCpuTime = nativeCpu;

    for (int i = 0; i < jdbcStats.length; i++) {
      jdbcStats[i] = new MethodStats();
    }
  }

  public static int registerMethod(String className, String methodName, String methodDescriptor) {
    String key = className.replace('/', '.') + "." + methodName + methodDescriptor;
    Integer existing = methodIdsByKey.get(key);
    if (existing != null) {
      return existing;
    }
    synchronized (Profiler.class) {
      existing = methodIdsByKey.get(key);
      if (existing != null) {
        return existing;
      }
      int id = nextMethodId.getAndIncrement();
      ensureMethodCapacity(id + 1);
      methodMetas[id] = new MethodMeta(className, methodName, methodDescriptor);
      methodStats[id] = new MethodStats();
      methodIdsByKey.put(key, id);
      return id;
    }
  }

  public static void setInstrumentation(java.lang.instrument.Instrumentation inst) {
    instrumentation = inst;
  }

  private static void ensureMethodCapacity(int required) {
    if (required <= methodMetas.length) {
      return;
    }
    int newLength = methodMetas.length;
    while (newLength < required) {
      newLength *= 2;
    }
    MethodMeta[] newMetas = new MethodMeta[newLength];
    MethodStats[] newStats = new MethodStats[newLength];
    System.arraycopy(methodMetas, 0, newMetas, 0, methodMetas.length);
    System.arraycopy(methodStats, 0, newStats, 0, methodStats.length);
    methodMetas = newMetas;
    methodStats = newStats;
  }

  public static void recordMethodEnter(int methodId) {
    if (!JavaAgent.isRecording()
        || methodId < 0
        || methodId >= methodStats.length
        || methodStats[methodId] == null) {
      return;
    }

    LocalCounters counters = localCounters.get();
    incrementInvocationCount(counters, methodId);
    boolean shouldSample = shouldTakeTimingSample(counters);
    if (!shouldSample) {
      counters.unsampledDepth++;
      return;
    }

    counters.sampledDepth++;
    stack.get().push(methodId, nowNano(), true, false, true);
  }

  private static boolean shouldTakeTimingSample(LocalCounters counters) {
    if (!samplingMode) {
      updateCallRate(counters);
      return true;
    }
    counters.sampleCounter++;
    if (counters.sampleCounter >= SAMPLING_INTERVAL) {
      counters.sampleCounter = 0;
      return true;
    }
    return false;
  }

  private static void updateCallRate(LocalCounters counters) {
    counters.rateCount++;
    if ((counters.rateCount & RATE_CHECK_MASK) != 0) {
      return;
    }
    long now = System.currentTimeMillis();
    if (counters.rateWindowStartedAtMs == 0) {
      counters.rateWindowStartedAtMs = now;
    }
    long elapsed = now - counters.rateWindowStartedAtMs;
    if (elapsed >= RATE_WINDOW_MS) {
      long callsPerSec = counters.rateCount * 1000 / elapsed;
      if (callsPerSec >= CALL_RATE_THRESHOLD) {
        samplingMode = true;
      }
      counters.rateCount = 0;
      counters.rateWindowStartedAtMs = now;
    }
  }

  public static void recordMethodExit(int methodId) {
    if (!JavaAgent.isRecording()
        || methodId < 0
        || methodId >= methodStats.length
        || methodStats[methodId] == null) {
      return;
    }
    recordExit(methodId);
  }

  public static void recordMethodEnter(
      String className, String methodName, String methodDescriptor) {
    if (!JavaAgent.isRecording()) {
      return;
    }
    recordMethodEnter(registerMethod(className, methodName, methodDescriptor));
  }

  public static void recordMethodExit(
      String className, String methodName, String methodDescriptor) {
    recordMethodExit(registerMethod(className, methodName, methodDescriptor));
  }

  public static void recordJdbcEnter(String className, String methodName, String methodDescriptor) {
    if (!JavaAgent.isRecording()) {
      return;
    }
    int methodId = registerMethod(className, methodName, methodDescriptor);
    incrementInvocationCount(methodId);
    stack.get().push(methodId, nowNano(), false, false, true);
  }

  public static void recordJdbcExit(String className, String methodName, String methodDescriptor) {
    recordMethodExit(registerMethod(className, methodName, methodDescriptor));
  }

  public static void recordSqlEnter(String sql) {
    if (!JavaAgent.isRecording() || sql == null || sql.isBlank()) {
      return;
    }
    recordJdbcEnter("SQL", sql, "");
  }

  public static void recordSqlExit(String sql) {
    if (!JavaAgent.isRecording() || sql == null || sql.isBlank()) {
      return;
    }
    recordJdbcExit("SQL", sql, "");
  }

  public static void recordAllocationTraceEnter(int methodId, boolean allocationBoundary) {
    if (!JavaAgent.isRecording()
        || methodId < 0
        || methodId >= methodStats.length
        || methodStats[methodId] == null) {
      return;
    }
    stack.get().push(methodId, 0, false, allocationBoundary, true);
  }

  public static void recordAllocationTraceExit(int methodId) {
    if (!JavaAgent.isRecording()) {
      return;
    }
    MethodStack s = stack.get();
    int topMethodId = s.popMethodId();
    if (topMethodId == Integer.MIN_VALUE || topMethodId == methodId) {
      return;
    }
    int match = s.findFromTop(methodId);
    if (match >= 0) {
      s.truncate(match);
    }
  }

  public static void recordJdbcProbeEnter(int probeId) {
    if (!JavaAgent.isRecording() || probeId < 0 || probeId >= jdbcStats.length) {
      return;
    }
    jdbcProbeStack.get().push(probeId, nowNano());
  }

  public static void recordJdbcProbeExit(int probeId) {
    if (!JavaAgent.isRecording() || probeId < 0 || probeId >= jdbcStats.length) {
      return;
    }
    JdbcProbeStack s = jdbcProbeStack.get();
    long startNano = s.pop(probeId);
    if (startNano < 0) {
      return;
    }
    long duration = nowNano() - startNano;
    if (duration < 0) {
      duration = 0;
    }
    duration = Math.max(0, duration - JavaAgent.jdbcProbeOverheadNanos(probeId));
    MethodStats ms = jdbcStats[probeId];
    ms.totalDurationNano.addAndGet(duration);
    ms.selfDurationNano.addAndGet(duration);
    ms.invocationCount.incrementAndGet();
  }

  public static long getJdbcProbeAverageDurationNanos(int probeId) {
    if (probeId < 0 || probeId >= jdbcStats.length) {
      return 0;
    }
    MethodStats ms = jdbcStats[probeId];
    long invocations = ms.invocationCount.get();
    if (invocations <= 0) {
      return 0;
    }
    return ms.selfDurationNano.get() / invocations;
  }

  public static void beginOverrideThreadStatus(int status) {
    if (!JavaAgent.isRecording()
        || overrideStartedAtNano.get() != null
        || status != AgentConstants.TIME_TYPE_RUNNING) {
      return;
    }
    overrideStartedAtNano.set(System.nanoTime());
  }

  public static void endOverrideThreadStatus() {
    Long startedAt = overrideStartedAtNano.get();
    if (startedAt == null) {
      return;
    }
    overrideStartedAtNano.remove();
    long duration = System.nanoTime() - startedAt;
    if (duration <= 0) {
      return;
    }
    MethodStack s = stack.get();
    if (!s.isEmpty()) {
      s.markTopTransparentToParent();
      s.addOverrideDurationToAll(duration);
    }
  }

  private static void recordExit(int methodId) {
    if (!JavaAgent.isRecording()) {
      return;
    }
    LocalCounters counters = localCounters.get();
    if (counters.unsampledDepth > 0 && counters.sampledDepth == 0) {
      counters.unsampledDepth--;
      return;
    }
    MethodStack s = stack.get();
    int entryIndex = s.topIndex();
    if (entryIndex < 0) {
      if (counters.unsampledDepth > 0) {
        counters.unsampledDepth--;
      }
      return;
    }
    if (s.methodIdAt(entryIndex) != methodId) {
      if (counters.unsampledDepth > 0) {
        counters.unsampledDepth--;
      }
      return;
    }

    boolean sampled = s.sampledAt(entryIndex);
    long startNano = s.startNanoAt(entryIndex);
    if (!sampled || startNano == 0) {
      s.truncate(entryIndex);
      if (counters.sampledDepth > 0) {
        counters.sampledDepth--;
      }
      return;
    }

    long duration = nowNano() - startNano + s.overrideDurationAt(entryIndex);
    long selfDuration =
        s.subtractChildrenAt(entryIndex) ? duration - s.childDurationAt(entryIndex) : duration;
    if (selfDuration < 0) {
      selfDuration = 0;
    }

    // Optimized: accumulate in thread-local buffers (no CAS)
    counters.ensureCapacity(methodId + 1);
    counters.selfDuration[methodId] += selfDuration;
    counters.totalDuration[methodId] += duration;

    boolean transparentToParent = s.transparentToParentAt(entryIndex);
    s.truncate(entryIndex);
    if (counters.sampledDepth > 0) {
      counters.sampledDepth--;
    }
    if (!s.isEmpty()) {
      int parentIndex = s.topIndex();
      if (!transparentToParent) {
        s.addChildDuration(parentIndex, duration);
      }
      if (edgeTrackingEnabled) {
        long edgeKey = edgeKey(s.methodIdAt(parentIndex), methodId);
        EdgeStats edge = edgeStats.computeIfAbsent(edgeKey, k -> new EdgeStats());
        edge.callCount.incrementAndGet();
        edge.totalDurationNano.addAndGet(duration);
      }
    }
  }

  private static void incrementInvocationCount(int methodId) {
    incrementInvocationCount(localCounters.get(), methodId);
  }

  private static void incrementInvocationCount(LocalCounters counters, int methodId) {
    counters.ensureCapacity(methodId + 1);
    counters.invocationCount[methodId]++;
  }

  private static long edgeKey(int fromMethodId, int toMethodId) {
    return ((long) fromMethodId << 32) | (toMethodId & 0xffff_ffffL);
  }

  private static int edgeFrom(long edgeKey) {
    return (int) (edgeKey >> 32);
  }

  private static int edgeTo(long edgeKey) {
    return (int) edgeKey;
  }

  public static java.util.List<MethodSnapshot> getMethodSnapshots() {
    flushThreadLocalBuffers();
    java.util.ArrayList<MethodSnapshot> snapshots = new java.util.ArrayList<>();
    MethodMeta[] metas = methodMetas;
    MethodStats[] currentStats = methodStats;
    int max = Math.min(nextMethodId.get(), Math.min(metas.length, currentStats.length));
    for (int i = 0; i < max; i++) {
      MethodMeta meta = metas[i];
      MethodStats stats = currentStats[i];
      if (meta == null || stats == null || stats.invocationCount.get() == 0) {
        continue;
      }
      snapshots.add(new MethodSnapshot(i, meta, stats));
    }
    return snapshots;
  }

  public static java.util.List<EdgeSnapshot> getEdgeSnapshots() {
    java.util.ArrayList<EdgeSnapshot> snapshots = new java.util.ArrayList<>();
    for (java.util.Map.Entry<Long, EdgeStats> entry : edgeStats.entrySet()) {
      EdgeStats stats = entry.getValue();
      if (stats.callCount.get() == 0) {
        continue;
      }
      long key = entry.getKey();
      snapshots.add(new EdgeSnapshot(edgeFrom(key), edgeTo(key), stats));
    }
    return snapshots;
  }

  public static java.util.List<AllocationSnapshot> getAllocationSnapshots() {
    java.util.HashMap<String, java.util.ArrayList<String>> childrenByParent =
        new java.util.HashMap<>();
    for (java.util.Map.Entry<String, AllocationStats> entry : allocationStats.entrySet()) {
      childrenByParent
          .computeIfAbsent(entry.getValue().parentPath, ignored -> new java.util.ArrayList<>())
          .add(entry.getKey());
    }
    for (java.util.ArrayList<String> children : childrenByParent.values()) {
      children.sort(
          (a, b) -> {
            AllocationStats left = allocationStats.get(a);
            AllocationStats right = allocationStats.get(b);
            long leftBytes = left == null ? 0 : liveTotals(left).size;
            long rightBytes = right == null ? 0 : liveTotals(right).size;
            return Long.compare(rightBytes, leftBytes);
          });
    }

    java.util.ArrayList<AllocationSnapshot> snapshots = new java.util.ArrayList<>();
    appendAllocationSnapshots("", -1, childrenByParent, snapshots);
    return snapshots;
  }

  private static void appendAllocationSnapshots(
      String parentPath,
      int parentId,
      java.util.Map<String, java.util.ArrayList<String>> childrenByParent,
      java.util.ArrayList<AllocationSnapshot> snapshots) {
    java.util.ArrayList<String> children = childrenByParent.get(parentPath);
    if (children == null) {
      return;
    }
    for (String path : children) {
      AllocationStats stats = allocationStats.get(path);
      if (stats == null) {
        continue;
      }
      LiveAllocationTotals liveTotals = liveTotals(stats);
      if (liveTotals.count == 0) {
        continue;
      }
      MethodMeta meta = null;
      MethodMeta[] metas = methodMetas;
      if (stats.methodId >= 0 && stats.methodId < metas.length) {
        meta = metas[stats.methodId];
      }
      int id = snapshots.size();
      snapshots.add(
          new AllocationSnapshot(
              id, parentId, meta, stats.allocatedType, liveTotals.size, liveTotals.count));
      appendAllocationSnapshots(path, id, childrenByParent, snapshots);
    }
  }

  public static ConcurrentHashMap<String, MethodStats> getStats() {
    flushThreadLocalBuffers();
    ConcurrentHashMap<String, MethodStats> snapshot = new ConcurrentHashMap<>(stats);
    MethodMeta[] metas = methodMetas;
    MethodStats[] currentStats = methodStats;
    int max = Math.min(nextMethodId.get(), Math.min(metas.length, currentStats.length));
    for (int i = 0; i < max; i++) {
      MethodMeta meta = metas[i];
      MethodStats source = currentStats[i];
      if (meta == null || source == null || source.invocationCount.get() == 0) {
        continue;
      }
      MethodStats copy = new MethodStats();
      copy.selfDurationNano.set(source.selfDurationNano.get());
      copy.totalDurationNano.set(source.totalDurationNano.get());
      copy.invocationCount.set(source.invocationCount.get());
      snapshot.put(meta.key, copy);
    }
    for (int i = 0; i < jdbcStats.length; i++) {
      MethodStats source = jdbcStats[i];
      if (source.invocationCount.get() == 0) {
        continue;
      }
      MethodStats copy = new MethodStats();
      copy.selfDurationNano.set(source.selfDurationNano.get());
      copy.totalDurationNano.set(source.totalDurationNano.get());
      copy.invocationCount.set(source.invocationCount.get());
      snapshot.put(JdbcProbes.key(i), copy);
    }
    return snapshot;
  }

  // Optimized: Flush thread-local accumulators to shared AtomicLong fields
  private static void flushThreadLocalBuffers() {
    for (LocalCounters counters : allLocalCounters) {
      flushLocalCounters(counters);
    }
  }

  private static void flushLocalCounters(LocalCounters counters) {
    long[] tlSelf = counters.selfDuration;
    long[] tlTotal = counters.totalDuration;
    long[] tlInv = counters.invocationCount;
    int maxLen = Math.min(tlSelf.length, Math.min(tlTotal.length, tlInv.length));

    MethodStats[] currentStats = methodStats;
    for (int i = 0; i < maxLen; i++) {
      if (tlInv[i] == 0) continue;
      if (i >= currentStats.length || currentStats[i] == null) continue;
      MethodStats ms = currentStats[i];
      ms.selfDurationNano.addAndGet(tlSelf[i]);
      ms.totalDurationNano.addAndGet(tlTotal[i]);
      ms.invocationCount.addAndGet(tlInv[i]);
      tlSelf[i] = 0;
      tlTotal[i] = 0;
      tlInv[i] = 0;
    }
  }

  public static void reset() {
    flushThreadLocalBuffers();
    stats.clear();
    edgeStats.clear();
    allocationStats.clear();
    MethodStats[] currentStats = methodStats;
    int max = Math.min(nextMethodId.get(), currentStats.length);
    for (int i = 0; i < max; i++) {
      MethodStats ms = currentStats[i];
      if (ms != null) {
        ms.selfDurationNano.set(0);
        ms.totalDurationNano.set(0);
        ms.invocationCount.set(0);
      }
    }
    for (MethodStats ms : jdbcStats) {
      ms.selfDurationNano.set(0);
      ms.totalDurationNano.set(0);
      ms.invocationCount.set(0);
    }
    samplingMode = DEFAULT_SAMPLING_MODE;
    for (LocalCounters counters : allLocalCounters) {
      counters.sampleCounter = 0;
      counters.rateCount = 0;
      counters.rateWindowStartedAtMs = 0;
      counters.unsampledDepth = 0;
      counters.sampledDepth = 0;
    }
  }

  public static void setSamplingMode(boolean enabled) {
    samplingMode = enabled;
    if (!enabled) {
      LocalCounters counters = localCounters.get();
      counters.sampleCounter = 0;
      counters.rateCount = 0;
      counters.rateWindowStartedAtMs = 0;
    }
  }

  public static boolean isSamplingMode() {
    return samplingMode;
  }

  public static long getSamplingCallRateThreshold() {
    return CALL_RATE_THRESHOLD;
  }

  public static int getSamplingInterval() {
    return SAMPLING_INTERVAL;
  }

  public static void setEdgeTrackingEnabled(boolean enabled) {
    edgeTrackingEnabled = enabled;
  }

  public static boolean isEdgeTrackingEnabled() {
    return edgeTrackingEnabled;
  }

  public static void objectAllocated(Object object, String type) {
    objectAllocatedAt(object, type, -1);
  }

  public static void beginAllocationRecordingSuppression() {
    allocationRecorderActive.set(true);
  }

  public static void endAllocationRecordingSuppression() {
    allocationRecorderActive.set(false);
  }

  public static void objectAllocatedAt(Object object, String type, int allocationMethodId) {
    if (!JavaAgent.isRecording() || object == null) {
      return;
    }
    if (allocationRecorderActive.get()) {
      return;
    }
    allocationRecorderActive.set(true);
    try {
      recordObjectAllocatedAt(object, type, allocationMethodId);
    } finally {
      allocationRecorderActive.set(false);
    }
  }

  private static void recordObjectAllocatedAt(Object object, String type, int allocationMethodId) {
    MethodStack currentStack = stack.get();
    if (currentStack.isEmpty() && allocationMethodId < 0) {
      return;
    }
    String allocatedType = normalizeAllocatedType(type);
    long size = objectSize(object);
    String parentPath = "";
    StackFrame[] frames = currentStack.framesFromTop();
    int rootIndex = -1;
    for (int i = 0; i < frames.length; i++) {
      if (frames[i].allocationBoundary) {
        rootIndex = i;
        break;
      }
    }
    if (rootIndex >= 0) {
      if (allocatedType.endsWith("[]")) {
        return;
      }
      int rootMethodId = frames[rootIndex].methodId;
      if (isNoisyBoundaryAllocationRoot(rootMethodId)) {
        return;
      }
      parentPath = addAllocationPathSegment(parentPath, rootMethodId, allocatedType, size, object);
      for (int i = rootIndex + 1; i < frames.length; i++) {
        int methodId = frames[i].methodId;
        if (methodId == rootMethodId) {
          continue;
        }
        parentPath = addAllocationPathSegment(parentPath, methodId, allocatedType, size, object);
      }
      return;
    }
    if (isInternalBackingArrayAllocation(allocationMethodId, allocatedType)) {
      return;
    }
    if (isNoisyInternalAllocationRoot(allocationMethodId, allocatedType)) {
      return;
    }
    if (allocationMethodId >= 0) {
      parentPath =
          addAllocationPathSegment(parentPath, allocationMethodId, allocatedType, size, object);
    }
    for (StackFrame frame : frames) {
      if (frame.methodId == allocationMethodId) {
        continue;
      }
      parentPath =
          addAllocationPathSegment(parentPath, frame.methodId, allocatedType, size, object);
    }
  }

  private static String addAllocationPathSegment(
      String parentPath, int methodId, String allocatedType, long size, Object object) {
    String path = parentPath.isEmpty() ? String.valueOf(methodId) : parentPath + ">" + methodId;
    final String statsParentPath = parentPath;
    AllocationStats stats =
        allocationStats.computeIfAbsent(
            path, ignored -> new AllocationStats(methodId, statsParentPath, ""));
    stats.allocatedSize.addAndGet(size);
    stats.instanceCount.incrementAndGet();
    stats.liveSamples.add(new AllocationSample(object, size));
    return path;
  }

  private static boolean isInternalBackingArrayAllocation(int methodId, String allocatedType) {
    if (!allocatedType.endsWith("[]") || methodId < 0 || methodId >= methodMetas.length) {
      return false;
    }
    MethodMeta meta = methodMetas[methodId];
    if (meta == null) {
      return false;
    }
    String className = meta.className;
    if (className.startsWith("com.ejt.") || className.startsWith("com.openprofiler.")) {
      return false;
    }
    if (meta.methodName.equals("readLine")
        || meta.methodName.equals("split")
        || meta.methodName.equals("getBytes")) {
      return false;
    }
    return className.startsWith("java.")
        || className.startsWith("javax.")
        || className.startsWith("sun.")
        || className.startsWith("com.sun.")
        || className.startsWith("jdk.");
  }

  private static boolean isNoisyInternalAllocationRoot(int methodId, String allocatedType) {
    if (methodId < 0 || methodId >= methodMetas.length) {
      return false;
    }
    MethodMeta meta = methodMetas[methodId];
    if (meta == null) {
      return false;
    }
    String method = meta.className + "." + meta.methodName;
    if (method.equals("sun.net.www.protocol.http.Handler.openConnection")
        && allocatedType.equals("sun.net.www.protocol.http.HttpURLConnection")) {
      return true;
    }
    if (method.startsWith("com.sun.net.httpserver.Headers.")
        || method.startsWith("sun.net.www.MessageHeader.")
        || method.startsWith("java.util.LinkedList.")) {
      return true;
    }
    return method.startsWith("sun.net.www.protocol.http.HttpURLConnection.<init>")
        || method.startsWith("sun.net.httpserver.ServerImpl$Exchange.")
        || method.startsWith("sun.net.httpserver.ServerImpl$Dispatcher.");
  }

  private static boolean isNoisyBoundaryAllocationRoot(int methodId) {
    if (methodId < 0 || methodId >= methodMetas.length) {
      return false;
    }
    MethodMeta meta = methodMetas[methodId];
    if (meta == null) {
      return false;
    }
    return meta.className.startsWith("java.util.concurrent.ThreadPoolExecutor");
  }

  private static LiveAllocationTotals liveTotals(AllocationStats stats) {
    long size = 0;
    long count = 0;
    for (AllocationSample sample : stats.liveSamples) {
      if (sample.reference.get() != null) {
        size += sample.size;
        count++;
      }
    }
    return new LiveAllocationTotals(size, count);
  }

  private static String normalizeAllocatedType(String type) {
    if (type == null || type.isEmpty()) {
      return "<unknown>";
    }
    if (type.startsWith("[")) {
      return arrayTypeName(type);
    }
    return type.replace('/', '.');
  }

  private static String arrayTypeName(String descriptor) {
    int depth = 0;
    while (depth < descriptor.length() && descriptor.charAt(depth) == '[') {
      depth++;
    }
    String base = descriptor.substring(depth);
    String name;
    if (base.startsWith("L") && base.endsWith(";")) {
      name = base.substring(1, base.length() - 1).replace('/', '.');
    } else {
      switch (base) {
        case "Z":
          name = "boolean";
          break;
        case "C":
          name = "char";
          break;
        case "F":
          name = "float";
          break;
        case "D":
          name = "double";
          break;
        case "B":
          name = "byte";
          break;
        case "S":
          name = "short";
          break;
        case "I":
          name = "int";
          break;
        case "J":
          name = "long";
          break;
        default:
          name = base;
          break;
      }
    }
    return name + "[]".repeat(Math.max(0, depth));
  }

  private static long objectSize(Object object) {
    try {
      java.lang.instrument.Instrumentation inst = instrumentation;
      if (inst != null) {
        long size = inst.getObjectSize(object);
        if (size > 0) {
          return size;
        }
      }
    } catch (Throwable ignored) {
    }
    return 16;
  }

  private static boolean initCpuTime() {
    try {
      if (threadBean.isThreadCpuTimeSupported()) {
        if (!threadBean.isThreadCpuTimeEnabled()) {
          threadBean.setThreadCpuTimeEnabled(true);
        }
        return threadBean.isThreadCpuTimeEnabled();
      }
    } catch (SecurityException | UnsupportedOperationException ignored) {
    }
    return false;
  }

  // Optimized: static final flags allow JIT to eliminate dead branches
  private static long nowNano() {
    if (useNativeCycleTime) {
      return Agent.currentThreadCpuCycleTimeNanos();
    }
    if (useNativeCpuTime) {
      return Agent.currentThreadCpuTimeNanos();
    }
    if (cpuTimeAvailable) {
      return threadBean.getCurrentThreadCpuTime();
    }
    return System.nanoTime();
  }

  private static final class JdbcProbeStack {
    private int[] probeIds = new int[32];
    private long[] startNanos = new long[32];
    private int depth = 0;

    void push(int probeId, long startNano) {
      if (depth == probeIds.length) {
        int newLength = probeIds.length * 2;
        int[] newProbeIds = new int[newLength];
        long[] newStartNanos = new long[newLength];
        System.arraycopy(probeIds, 0, newProbeIds, 0, probeIds.length);
        System.arraycopy(startNanos, 0, newStartNanos, 0, startNanos.length);
        probeIds = newProbeIds;
        startNanos = newStartNanos;
      }
      probeIds[depth] = probeId;
      startNanos[depth] = startNano;
      depth++;
    }

    long pop(int probeId) {
      if (depth == 0) {
        return -1;
      }
      depth--;
      if (probeIds[depth] != probeId) {
        return -1;
      }
      return startNanos[depth];
    }
  }
}
