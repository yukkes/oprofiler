package com.openprofiler.benchmark;

/**
 * Benchmark for loop-heavy workloads where one profiled method performs a large amount of work
 * internally. This complements StandaloneBenchmark, which intentionally stresses per-call
 * instrumentation overhead.
 */
public class LargeLoopBenchmark {
  private static final int LOOP_COUNT = 100_000_000; // 10^8
  private static final int WARMUP_COUNT = 10_000_000;
  private static final int ITERATIONS = 3;

  private static volatile long sink = 0;

  public static void main(String[] args) throws Exception {
    System.out.println("========================================");
    System.out.println(" OpenProfiler Large Loop Benchmark");
    System.out.println("========================================");
    System.out.println("Loop iterations per phase: " + LOOP_COUNT);
    System.out.println("Iterations per phase: " + ITERATIONS);
    System.out.println();

    boolean agentAvailable = isAgentAvailable();
    System.out.println("Agent: " + (agentAvailable ? "AVAILABLE" : "NOT AVAILABLE"));
    System.out.println();

    System.out.println("--- Warmup ---");
    for (int i = 0; i < 5; i++) {
      measurePhase("Warmup", WARMUP_COUNT, 1);
    }
    System.out.println();

    System.out.println("--- Phase 1: Idle (not recording) ---");
    double idleNsPerLoop = measurePhase("Idle", LOOP_COUNT, ITERATIONS);
    System.out.println();

    if (!agentAvailable) {
      System.out.println("Agent not available. Run with -javaagent to measure profiling overhead.");
      return;
    }

    System.out.println("--- Phase 2: Profiling (recording) ---");
    startRecording();
    Thread.sleep(200);
    double profilingNsPerLoop = measurePhase("Profiling", LOOP_COUNT, ITERATIONS);
    stopRecording();
    Thread.sleep(100);
    System.out.println();

    System.out.println("--- Phase 3: Idle (after recording stopped) ---");
    double idleAfterNsPerLoop = measurePhase("Idle-after", LOOP_COUNT, ITERATIONS);
    System.out.println();

    System.out.println("========================================");
    System.out.println(" Summary (ns per loop iteration)");
    System.out.println("========================================");
    System.out.printf("Idle (not recording):      %8.2f ns/iter%n", idleNsPerLoop);
    System.out.printf(
        "Profiling (recording):     %8.2f ns/iter (%.2fx)%n",
        profilingNsPerLoop, profilingNsPerLoop / Math.max(idleNsPerLoop, 0.01));
    System.out.printf(
        "Idle (after recording):    %8.2f ns/iter (%.2fx idle)%n",
        idleAfterNsPerLoop, idleAfterNsPerLoop / Math.max(idleNsPerLoop, 0.01));

    double profilingOverhead = profilingNsPerLoop - idleNsPerLoop;
    double residualOverhead = idleAfterNsPerLoop - idleNsPerLoop;
    double residualRatio = idleAfterNsPerLoop / Math.max(idleNsPerLoop, 0.01);
    System.out.printf("%nProfiling overhead:        %8.2f ns/iter%n", profilingOverhead);
    System.out.printf("Residual overhead:         %8.2f ns/iter%n", residualOverhead);
    System.out.printf("Residual ratio:            %8.2fx%n", residualRatio);
    System.out.println(
        residualRatio > 1.1
            ? "WARNING: Residual overhead detected!"
            : "OK: No significant residual overhead");
  }

  private static boolean isAgentAvailable() {
    try {
      Class.forName("com.openprofiler.agent.JavaAgent");
      return true;
    } catch (ClassNotFoundException e) {
      return false;
    }
  }

  private static double measurePhase(String label, int count, int iterations) {
    double totalNs = 0;
    for (int iter = 0; iter < iterations; iter++) {
      System.gc();
      try {
        Thread.sleep(30);
      } catch (InterruptedException ignored) {
      }

      long start = System.nanoTime();
      long value = runLoop(count);
      long elapsed = System.nanoTime() - start;
      sink ^= value;

      double nsPerIteration = (double) elapsed / count;
      totalNs += nsPerIteration;
      System.out.printf(
          "  Iteration %d: %6.0f ms (%.2f ns/iter)%n",
          iter + 1, elapsed / 1_000_000.0, nsPerIteration);
    }
    double average = totalNs / iterations;
    System.out.printf("  Average: %.2f ns/iter%n", average);
    return average;
  }

  private static long runLoop(int count) {
    long local = sink;
    for (int i = 0; i < count; i++) {
      local += (i * 31L + 17L) ^ (local >>> 7);
    }
    return local;
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
