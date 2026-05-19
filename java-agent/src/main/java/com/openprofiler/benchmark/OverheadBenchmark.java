package com.openprofiler.benchmark;

/**
 * Benchmark application to measure profiler overhead.
 *
 * <p>Usage: Without agent (baseline): java -cp . com.openprofiler.benchmark.OverheadBenchmark
 *
 * <p>With agent (profiling overhead test): java -javaagent:java-agent-fat.jar=port=8849 -cp .
 * com.openprofiler.benchmark.OverheadBenchmark
 *
 * <p>The benchmark runs three phases: 1. Idle (no recording) - 10^8 calls 2. Profiling (recording)
 * - 10^8 calls 3. Idle again (after stop) - 10^8 calls
 *
 * <p>Results are printed to stdout.
 */
public class OverheadBenchmark {

  private static final int CALL_COUNT = 100_000_000; // 10^8
  private static final int WARMUP_COUNT = 1_000_000; // 10^6

  // Volatile flag to prevent JIT from optimizing away the loop
  private static volatile long sink = 0;

  public static void main(String[] args) throws Exception {
    System.out.println("=== OpenProfiler Overhead Benchmark ===");
    System.out.println("Call count per phase: " + CALL_COUNT);
    System.out.println();

    // Warmup
    System.out.println("Warming up...");
    runBenchmark(WARMUP_COUNT, "warmup");
    System.out.println("Warmup complete.");
    System.out.println();

    // Phase 1: Idle (baseline)
    long idleTime = runBenchmark(CALL_COUNT, "Idle (baseline)");
    double idleNsPerCall = (double) idleTime / CALL_COUNT;
    System.out.printf(
        "Phase 1 - Idle: %.2f ms (%.2f ns/call)%n", idleTime / 1_000_000.0, idleNsPerCall);
    System.out.println();

    // Phase 2: Profiling
    System.out.println("Starting CPU recording...");
    try {
      com.openprofiler.agent.JavaAgent.startRecording();
      System.out.println("CPU recording started.");
    } catch (Throwable e) {
      System.out.println("Agent not available, skipping profiling phase.");
      System.out.println("Run with -javaagent:java-agent-fat.jar to test profiling overhead.");
      return;
    }

    // Small delay to let sampling mode auto-detect if needed
    Thread.sleep(100);

    long profilingTime = runBenchmark(CALL_COUNT, "Profiling");
    double profilingNsPerCall = (double) profilingTime / CALL_COUNT;
    System.out.printf(
        "Phase 2 - Profiling: %.2f ms (%.2f ns/call)%n",
        profilingTime / 1_000_000.0, profilingNsPerCall);

    boolean samplingMode = com.openprofiler.agent.Profiler.isSamplingMode();
    System.out.println("  Sampling mode: " + samplingMode);
    if (samplingMode) {
      System.out.println(
          "  Sampling interval: 1/" + com.openprofiler.agent.Profiler.getSamplingInterval());
      System.out.println(
          "  Call rate threshold: "
              + com.openprofiler.agent.Profiler.getSamplingCallRateThreshold()
              + " calls/sec");
    }
    System.out.println();

    // Phase 3: Idle again (after stop)
    System.out.println("Stopping CPU recording...");
    com.openprofiler.agent.JavaAgent.stopRecording();
    System.out.println("CPU recording stopped.");
    System.out.println();

    long idleAfterTime = runBenchmark(CALL_COUNT, "Idle (after profiling)");
    double idleAfterNsPerCall = (double) idleAfterTime / CALL_COUNT;
    System.out.printf(
        "Phase 3 - Idle (after profiling): %.2f ms (%.2f ns/call)%n",
        idleAfterTime / 1_000_000.0, idleAfterNsPerCall);
    System.out.println();

    // Summary
    System.out.println("=== Summary ===");
    System.out.printf("Baseline (Idle):           %.2f ns/call%n", idleNsPerCall);
    System.out.printf(
        "Profiling overhead:        %.2f ns/call (%.1fx slower)%n",
        profilingNsPerCall - idleNsPerCall, profilingNsPerCall / idleNsPerCall);
    System.out.printf(
        "Residual overhead (Idle):  %.2f ns/call (%.1fx baseline)%n",
        idleAfterNsPerCall - idleNsPerCall, idleAfterNsPerCall / idleNsPerCall);

    // Verify residual overhead is minimal
    double residualRatio = idleAfterNsPerCall / idleNsPerCall;
    if (residualRatio > 1.5) {
      System.out.println();
      System.out.println(
          "WARNING: Residual overhead detected! Idle after profiling is "
              + String.format("%.1fx", residualRatio)
              + " slower than baseline.");
      System.out.println("This indicates the profiler is not properly restoring JIT state.");
    } else {
      System.out.println();
      System.out.println("OK: No significant residual overhead detected.");
    }
  }

  private static long runBenchmark(int count, String label) {
    System.gc(); // Try to reduce GC noise
    try {
      Thread.sleep(50);
    } catch (InterruptedException e) {
    }

    long start = System.nanoTime();
    for (int i = 0; i < count; i++) {
      benchmarkMethod(i);
    }
    long end = System.nanoTime();
    long elapsed = end - start;

    System.out.printf("[%s] %d calls in %.2f ms%n", label, count, elapsed / 1_000_000.0);
    return elapsed;
  }

  // This method will be instrumented by the profiler
  private static long benchmarkMethod(int i) {
    // Simple operation to prevent JIT from eliminating the call
    sink += i;
    return sink;
  }
}
