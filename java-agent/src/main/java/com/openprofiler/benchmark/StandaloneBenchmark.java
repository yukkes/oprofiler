package com.openprofiler.benchmark;

/**
 * Benchmark to measure profiler overhead with accurate measurements. Uses multiple iterations to
 * verify consistency.
 */
public class StandaloneBenchmark {

  private static final int CALL_COUNT = 100_000_000; // 10^8
  private static final int WARMUP_COUNT = 5_000_000;
  private static final int ITERATIONS = 3;

  private static volatile long sink = 0;
  private static volatile boolean agentAvailable = false;

  interface Computable {
    long compute(int i);
  }

  public static void main(String[] args) throws Exception {
    System.out.println("========================================");
    System.out.println(" OpenProfiler Overhead Benchmark");
    System.out.println("========================================");
    System.out.println("Calls per phase: " + CALL_COUNT);
    System.out.println("Iterations per phase: " + ITERATIONS);
    System.out.println();

    try {
      Class.forName("com.openprofiler.agent.JavaAgent");
      agentAvailable = true;
      System.out.println("Agent: AVAILABLE");
    } catch (ClassNotFoundException e) {
      agentAvailable = false;
      System.out.println("Agent: NOT AVAILABLE");
    }
    System.out.println();

    Computable worker =
        new Computable() {
          @Override
          public long compute(int i) {
            return i * 31 + 17;
          }
        };

    // Warmup
    System.out.println("--- Warmup ---");
    for (int w = 0; w < 5; w++) {
      measurePhase("Warmup", WARMUP_COUNT, worker, 1);
    }
    System.out.println();

    // Phase 1: Idle (agent loaded, not recording)
    System.out.println("--- Phase 1: Idle (not recording) ---");
    double idleNsPerCall = measurePhase("Idle", CALL_COUNT, worker, ITERATIONS);
    System.out.println();

    if (!agentAvailable) {
      System.out.println("Agent not available. Run with -javaagent to measure profiling overhead.");
      return;
    }

    // Phase 2: Profiling
    System.out.println("--- Phase 2: Profiling (recording) ---");
    startRecording();
    Thread.sleep(200);
    double profilingNsPerCall = measurePhase("Profiling", CALL_COUNT, worker, ITERATIONS);

    try {
      Class<?> profilerClass = Class.forName("com.openprofiler.agent.Profiler");
      java.lang.reflect.Method isSampling = profilerClass.getMethod("isSamplingMode");
      boolean sampling = (Boolean) isSampling.invoke(null);
      System.out.printf("  Sampling mode: %s%n", sampling);
    } catch (Exception e) {
    }

    stopRecording();
    Thread.sleep(100);
    System.out.println();

    // Phase 3: Idle after profiling
    System.out.println("--- Phase 3: Idle (after recording stopped) ---");
    double idleAfterNsPerCall = measurePhase("Idle-after", CALL_COUNT, worker, ITERATIONS);
    System.out.println();

    // Summary
    System.out.println("========================================");
    System.out.println(" Summary (ns per call)");
    System.out.println("========================================");
    System.out.printf("Idle (not recording):      %8.2f ns/call%n", idleNsPerCall);
    System.out.printf(
        "Profiling (recording):     %8.2f ns/call (%.1fx)%n",
        profilingNsPerCall, profilingNsPerCall / Math.max(idleNsPerCall, 0.01));
    System.out.printf(
        "Idle (after recording):    %8.2f ns/call (%.2fx idle)%n",
        idleAfterNsPerCall, idleAfterNsPerCall / Math.max(idleNsPerCall, 0.01));

    double profilingOverhead = profilingNsPerCall - idleNsPerCall;
    double residualOverhead = idleAfterNsPerCall - idleNsPerCall;
    System.out.printf("%nProfiling overhead:        %8.2f ns/call%n", profilingOverhead);
    System.out.printf("Residual overhead:         %8.2f ns/call%n", residualOverhead);

    double residualRatio = idleAfterNsPerCall / Math.max(idleNsPerCall, 0.01);
    System.out.printf("Residual ratio:            %8.2fx%n", residualRatio);
    System.out.println();

    if (residualRatio > 1.1) {
      System.out.println("WARNING: Residual overhead detected!");
    } else {
      System.out.println("OK: No significant residual overhead");
    }
  }

  private static double measurePhase(String label, int count, Computable worker, int iterations) {
    double totalNs = 0;
    for (int iter = 0; iter < iterations; iter++) {
      System.gc();
      try {
        Thread.sleep(30);
      } catch (InterruptedException e) {
      }

      long localSink = 0;
      long start = System.nanoTime();
      for (int i = 0; i < count; i++) {
        localSink += worker.compute(i);
      }
      long elapsed = System.nanoTime() - start;
      sink += localSink;

      double nsPerCall = (double) elapsed / count;
      totalNs += nsPerCall;
      System.out.printf(
          "  Iteration %d: %6.0f ms (%.2f ns/call)%n", iter + 1, elapsed / 1_000_000.0, nsPerCall);
    }
    double avgNsPerCall = totalNs / iterations;
    System.out.printf("  Average: %.2f ns/call%n", avgNsPerCall);
    return avgNsPerCall;
  }

  private static void startRecording() throws Exception {
    Class<?> cls = Class.forName("com.openprofiler.agent.JavaAgent");
    cls.getMethod("startRecording").invoke(null);
    System.out.println("  Recording started.");
  }

  private static void stopRecording() throws Exception {
    Class<?> cls = Class.forName("com.openprofiler.agent.JavaAgent");
    cls.getMethod("stopRecording").invoke(null);
    System.out.println("  Recording stopped.");
  }
}
