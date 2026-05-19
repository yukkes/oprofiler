package com.openprofiler.agent;

public class Agent {
  static {
    String explicitPath = System.getProperty("openprofiler.native.path");
    try {
      if (explicitPath != null && !explicitPath.isBlank()) {
        System.load(explicitPath);
      } else {
        System.loadLibrary("jvmti_agent_rust");
      }
    } catch (UnsatisfiedLinkError ignored) {
    }
  }

  public static native void startRecording();

  public static native void stopRecording();

  public static native boolean isRecording();

  public static native long getTotalSamples();

  public static native long getSamplingIntervalMs();

  public static native void setSamplingIntervalMs(long intervalMs);

  public static native long currentThreadCpuTimeNanos();

  public static native long highResolutionTimeNanos();

  public static native long currentThreadCpuCycleTimeNanos();

  public static native void recordJdbcProbeEnter(int probeId);

  public static native void recordJdbcProbeExit(int probeId);

  public static native void resetNativeJdbcHotSpots();

  public static native long getNativeJdbcProbeAverageDurationNanos(int probeId);

  public static native String getNativeJdbcHotSpotsJson();

  public static native String getNativeAllocationHotSpotsTsv();

  public static native void resetNativeAllocationHotSpots();

  public static native void recordSample(
      String className, String methodName, String methodDescriptor, boolean isLeaf);

  public static native String getHotSpotsJson();
}
