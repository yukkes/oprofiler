package com.openprofiler.agent;

import java.io.File;
import java.lang.instrument.Instrumentation;
import java.util.jar.JarFile;

public class JavaAgent {

  private static Instrumentation instrumentation;
  private static volatile boolean recording = false;
  private static TcpServer tcpServer;
  private static volatile boolean nativeTimingAvailable = false;
  private static volatile String jdbcProbeMode =
      System.getProperty("openprofiler.jdbc.probe.mode", "native");
  private static volatile boolean jdbcProbeEnabled =
      Boolean.getBoolean("openprofiler.jdbc.probe.enabled");
  private static final int jdbcProbeCalibrationIterations =
      Integer.getInteger("openprofiler.jdbc.probe.calibration.iterations", 2000);
  private static final long[] jdbcProbeOverheadNanos = new long[JdbcProbes.COUNT];
  private static final Object recordingLock = new Object();

  public static void premain(String agentArgs, Instrumentation inst) {
    System.out.println("[java-agent] premain called");
    init(agentArgs, inst);
  }

  public static void agentmain(String agentArgs, Instrumentation inst) {
    System.out.println("[java-agent] agentmain called (dynamic attach)");
    try {
      init(agentArgs, inst);
    } catch (Throwable t) {
      System.err.println("[java-agent] agentmain failed: " + t);
      t.printStackTrace(System.err);
    }
  }

  private static void retransformLoadedApplicationClasses(Instrumentation inst) {
    if (!inst.isRetransformClassesSupported()) {
      return;
    }
    long started = System.nanoTime();
    int scanned = 0;
    int transformed = 0;
    int skipped = 0;
    for (Class<?> cls : inst.getAllLoadedClasses()) {
      scanned++;
      if (!ProfilingClassFileTransformer.shouldRetransformClass(cls)) {
        skipped++;
        continue;
      }
      try {
        inst.retransformClasses(cls);
        transformed++;
      } catch (Throwable t) {
        System.err.println("[java-agent] Retransform skipped for " + cls.getName() + ": " + t);
      }
    }
    long elapsedMs = (System.nanoTime() - started) / 1_000_000L;
    System.out.println(
        "[java-agent] Retransform scan completed: scanned="
            + scanned
            + ", transformed="
            + transformed
            + ", skipped="
            + skipped
            + ", elapsedMs="
            + elapsedMs);
  }

  private static void restoreTransformedApplicationClasses(Instrumentation inst) {
    if (inst == null || !inst.isRetransformClassesSupported()) {
      return;
    }
    long started = System.nanoTime();
    int scanned = 0;
    int restored = 0;
    int skipped = 0;
    for (Class<?> cls : inst.getAllLoadedClasses()) {
      scanned++;
      if (!ProfilingClassFileTransformer.shouldRestoreClass(cls)) {
        skipped++;
        continue;
      }
      try {
        inst.retransformClasses(cls);
        ProfilingClassFileTransformer.markRestored(cls);
        restored++;
      } catch (Throwable t) {
        System.err.println("[java-agent] Restore skipped for " + cls.getName() + ": " + t);
      }
    }
    long elapsedMs = (System.nanoTime() - started) / 1_000_000L;
    System.out.println(
        "[java-agent] Restore scan completed: scanned="
            + scanned
            + ", restored="
            + restored
            + ", skipped="
            + skipped
            + ", elapsedMs="
            + elapsedMs);
  }

  static boolean isModifiable(Class<?> cls) {
    try {
      Instrumentation inst = instrumentation;
      return inst != null && inst.isModifiableClass(cls);
    } catch (Throwable ignored) {
      return false;
    }
  }

  private static void init(String agentArgs, Instrumentation inst) {
    instrumentation = inst;
    int port = configureFromAgentArgs(agentArgs);
    appendSelfToBootstrap(inst);
    publishInstrumentationToBootstrap(inst);
    loadNativeTimingLibrary();
    publishInstrumentationToBootstrap(inst);
    preloadBootstrapHelpers();
    Profiler.setInstrumentation(inst);
    inst.addTransformer(new ProfilingClassFileTransformer(), true);
    tcpServer = new TcpServer(port);
    tcpServer.start();
  }

  private static void appendSelfToBootstrap(Instrumentation inst) {
    try {
      java.net.URL location = JavaAgent.class.getProtectionDomain().getCodeSource().getLocation();
      if (location == null) {
        System.out.println("[java-agent] Agent location unavailable, skipping bootstrap append");
        return;
      }
      String path = location.toURI().getPath();
      File jar = new File(path);
      if (jar.isFile()) {
        inst.appendToBootstrapClassLoaderSearch(new JarFile(jar));
        System.out.println("[java-agent] Agent jar appended to bootstrap search: " + jar);
      } else {
        System.out.println(
            "[java-agent] Agent not in jar (path: " + path + "), skipping bootstrap append");
      }
    } catch (Throwable t) {
      System.err.println("[java-agent] Could not append agent jar to bootstrap search: " + t);
    }
  }

  private static void publishInstrumentationToBootstrap(Instrumentation inst) {
    try {
      Class<?> bootstrapAgent = Class.forName("com.openprofiler.agent.JavaAgent", true, null);
      setBootstrapField(bootstrapAgent, "instrumentation", inst);
      setBootstrapField(bootstrapAgent, "nativeTimingAvailable", nativeTimingAvailable);
      setBootstrapField(bootstrapAgent, "jdbcProbeMode", jdbcProbeMode);
      setBootstrapField(bootstrapAgent, "jdbcProbeEnabled", jdbcProbeEnabled);
    } catch (Throwable t) {
      System.err.println("[java-agent] Could not publish instrumentation to bootstrap agent: " + t);
    }
  }

  private static void setBootstrapField(Class<?> bootstrapAgent, String name, Object value)
      throws ReflectiveOperationException {
    java.lang.reflect.Field field = bootstrapAgent.getDeclaredField(name);
    field.setAccessible(true);
    field.set(null, value);
  }

  private static void preloadBootstrapHelpers() {
    try {
      Class.forName("com.openprofiler.agent.Profiler", true, JavaAgent.class.getClassLoader());
      Class.forName("com.openprofiler.agent.SqlHotSpots", true, JavaAgent.class.getClassLoader());
      Class.forName("com.openprofiler.agent.Agent", true, JavaAgent.class.getClassLoader());
    } catch (Throwable t) {
      System.err.println("[java-agent] Could not preload bootstrap helpers: " + t);
    }
  }

  private static int configureFromAgentArgs(String agentArgs) {
    int port = 8849;
    String portProp = System.getProperty("oprofiler.agent.port");
    if (portProp != null && !portProp.isBlank()) {
      try {
        port = Integer.parseInt(portProp.trim());
      } catch (NumberFormatException ignored) {
      }
    }
    if (agentArgs == null || agentArgs.isBlank()) {
      return port;
    }
    for (String token : agentArgs.split("[;,]")) {
      String trimmed = token.trim();
      if (trimmed.isEmpty()) {
        continue;
      }
      if (trimmed.startsWith("port=")) {
        try {
          port = Integer.parseInt(trimmed.substring("port=".length()).trim());
        } catch (NumberFormatException ignored) {
        }
      } else if (trimmed.startsWith("native=")) {
        System.setProperty(
            "openprofiler.native.path", trimmed.substring("native=".length()).trim());
      } else if (trimmed.startsWith("jdbc=")) {
        jdbcProbeEnabled = Boolean.parseBoolean(trimmed.substring("jdbc=".length()).trim());
      } else if (trimmed.startsWith("jdbcMode=")) {
        jdbcProbeMode = trimmed.substring("jdbcMode=".length()).trim();
      } else if (trimmed.startsWith("includes=")) {
        System.setProperty(
            "openprofiler.instrument.includes", trimmed.substring("includes=".length()).trim());
      } else if (trimmed.startsWith("excludes=")) {
        System.setProperty(
            "openprofiler.instrument.excludes", trimmed.substring("excludes=".length()).trim());
      } else {
        try {
          port = Integer.parseInt(trimmed);
        } catch (NumberFormatException ignored) {
        }
      }
    }
    return port;
  }

  private static void loadNativeTimingLibrary() {
    if (nativeTimingAvailable) {
      return;
    }
    try {
      Agent.isRecording();
      nativeTimingAvailable = true;
      System.out.println("[java-agent] Native timing loaded");
    } catch (UnsatisfiedLinkError e) {
      nativeTimingAvailable = false;
      System.err.println(
          "[java-agent] Native timing unavailable, falling back to Java ThreadMXBean: "
              + e.getMessage());
    }
  }

  public static boolean isNativeTimingAvailable() {
    return nativeTimingAvailable;
  }

  public static String getJdbcProbeMode() {
    return jdbcProbeMode;
  }

  public static boolean isJdbcProbeEnabled() {
    return jdbcProbeEnabled;
  }

  public static long jdbcProbeOverheadNanos(int probeId) {
    if (probeId < 0 || probeId >= jdbcProbeOverheadNanos.length) {
      return 0;
    }
    return jdbcProbeOverheadNanos[probeId];
  }

  public static Instrumentation getInstrumentation() {
    return instrumentation;
  }

  public static void startRecording() {
    synchronized (recordingLock) {
      long started = System.nanoTime();
      recording = true;
      try {
        Profiler.reset();
      } catch (Throwable t) {
        System.err.println("[java-agent] Profiler reset failed: " + t);
        t.printStackTrace(System.err);
      }
      try {
        SqlHotSpots.reset();
      } catch (Throwable t) {
        System.err.println("[java-agent] SQL hot spot reset failed: " + t);
        t.printStackTrace(System.err);
      }
      if (nativeTimingAvailable) {
        try {
          Agent.resetNativeJdbcHotSpots();
        } catch (Throwable t) {
          System.err.println("[java-agent] Native JDBC reset failed: " + t);
        }
      }
      try {
        Agent.startRecording();
        try {
          Agent.getNativeAllocationHotSpotsTsv();
          Agent.resetNativeAllocationHotSpots();
        } catch (Throwable ignored) {
        }
      } catch (Throwable t) {
        System.err.println("[java-agent] Native recording start failed: " + t);
      }
      retransformLoadedApplicationClasses(instrumentation);
      try {
        calibrateJdbcProbeOverhead();
      } catch (Throwable t) {
        System.err.println("[java-agent] JDBC probe calibration failed: " + t);
        t.printStackTrace(System.err);
      }
      long elapsedMs = (System.nanoTime() - started) / 1_000_000L;
      System.out.println("[java-agent] Recording started in " + elapsedMs + " ms");
    }
  }

  public static void stopRecording() {
    synchronized (recordingLock) {
      long started = System.nanoTime();
      Profiler.setSamplingMode(false);
      try {
        Agent.stopRecording();
      } catch (Throwable ignored) {
      }
      recording = false;
      restoreTransformedApplicationClasses(instrumentation);
      long elapsedMs = (System.nanoTime() - started) / 1_000_000L;
      System.out.println("[java-agent] Recording stopped in " + elapsedMs + " ms");
    }
  }

  public static boolean isRecording() {
    return recording;
  }

  private static void calibrateJdbcProbeOverhead() {
    clearJdbcProbeOverhead();
    if (!jdbcProbeEnabled) {
      return;
    }
    if (jdbcProbeCalibrationIterations <= 0) {
      return;
    }
    if ("native".equals(jdbcProbeMode) && nativeTimingAvailable) {
      calibrateNativeJdbcProbeOverhead();
    } else if ("id".equals(jdbcProbeMode)) {
      calibrateJavaJdbcProbeOverhead();
    } else {
      clearJdbcProbeOverhead();
    }
  }

  private static void clearJdbcProbeOverhead() {
    for (int i = 0; i < jdbcProbeOverheadNanos.length; i++) {
      jdbcProbeOverheadNanos[i] = 0;
    }
  }

  private static void calibrateNativeJdbcProbeOverhead() {
    try {
      Agent.resetNativeJdbcHotSpots();
      for (int probeId = 0; probeId < JdbcProbes.COUNT; probeId++) {
        for (int i = 0; i < jdbcProbeCalibrationIterations; i++) {
          Agent.recordJdbcProbeEnter(probeId);
          Agent.recordJdbcProbeExit(probeId);
        }
        jdbcProbeOverheadNanos[probeId] =
            Math.max(0, Agent.getNativeJdbcProbeAverageDurationNanos(probeId));
        Agent.resetNativeJdbcHotSpots();
      }
      System.out.println(
          "[java-agent] Native JDBC probe self-calibrated with "
              + jdbcProbeCalibrationIterations
              + " iterations");
    } catch (Throwable e) {
      clearJdbcProbeOverhead();
    } finally {
      try {
        Agent.resetNativeJdbcHotSpots();
      } catch (Throwable ignored) {
      }
    }
  }

  private static void calibrateJavaJdbcProbeOverhead() {
    Profiler.reset();
    for (int probeId = 0; probeId < JdbcProbes.COUNT; probeId++) {
      for (int i = 0; i < jdbcProbeCalibrationIterations; i++) {
        Profiler.recordJdbcProbeEnter(probeId);
        Profiler.recordJdbcProbeExit(probeId);
      }
      jdbcProbeOverheadNanos[probeId] =
          Math.max(0, Profiler.getJdbcProbeAverageDurationNanos(probeId));
      Profiler.reset();
    }
    System.out.println(
        "[java-agent] Java JDBC probe self-calibrated with "
            + jdbcProbeCalibrationIterations
            + " iterations");
  }
}
