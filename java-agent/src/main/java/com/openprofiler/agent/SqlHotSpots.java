package com.openprofiler.agent;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.Deque;
import java.util.List;
import java.util.Map;
import java.util.WeakHashMap;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;

public final class SqlHotSpots {
  public static final class Snapshot {
    public final String sql;
    public final long totalDurationNano;
    public final long invocations;

    Snapshot(String sql, SqlStats stats) {
      this.sql = sql;
      this.totalDurationNano = stats.totalDurationNano.get();
      this.invocations = stats.invocations.get();
    }
  }

  private static final class SqlStats {
    final AtomicLong totalDurationNano = new AtomicLong();
    final AtomicLong invocations = new AtomicLong();
  }

  private static final class SqlEntry {
    final String sql;
    final long startedNano;

    SqlEntry(String sql, long startedNano) {
      this.sql = sql;
      this.startedNano = startedNano;
    }
  }

  private static final ConcurrentHashMap<String, SqlStats> stats = new ConcurrentHashMap<>();
  private static final ThreadLocal<Deque<SqlEntry>> stack =
      ThreadLocal.withInitial(ArrayDeque::new);
  private static final Map<Object, String> preparedSql = new WeakHashMap<>();

  private SqlHotSpots() {}

  public static void reset() {
    stats.clear();
    synchronized (preparedSql) {
      preparedSql.clear();
    }
  }

  public static void enter(String sql) {
    if (!JavaAgent.isRecording() || sql == null || sql.isBlank()) {
      return;
    }
    stack.get().push(new SqlEntry(sql, System.nanoTime()));
    Profiler.recordSqlEnter(sql);
  }

  public static void enterPrepared(Object statement) {
    if (!JavaAgent.isRecording() || statement == null) {
      return;
    }
    String sql;
    synchronized (preparedSql) {
      sql = preparedSql.get(statement);
    }
    enter(sql);
  }

  public static void exit() {
    if (!JavaAgent.isRecording()) {
      return;
    }
    Deque<SqlEntry> entries = stack.get();
    SqlEntry entry = entries.poll();
    if (entry == null) {
      return;
    }
    long duration = Math.max(0, System.nanoTime() - entry.startedNano);
    SqlStats sqlStats = stats.computeIfAbsent(entry.sql, ignored -> new SqlStats());
    sqlStats.totalDurationNano.addAndGet(duration);
    sqlStats.invocations.incrementAndGet();
    Profiler.recordSqlExit(entry.sql);
  }

  public static void associatePreparedStatement(Object statement, String sql) {
    if (statement == null || sql == null || sql.isBlank()) {
      return;
    }
    synchronized (preparedSql) {
      preparedSql.put(statement, sql);
    }
  }

  public static List<Snapshot> snapshots() {
    List<Snapshot> rows = new ArrayList<>();
    for (Map.Entry<String, SqlStats> entry : stats.entrySet()) {
      SqlStats value = entry.getValue();
      if (value.invocations.get() == 0) {
        continue;
      }
      rows.add(new Snapshot(entry.getKey(), value));
    }
    rows.sort(Comparator.comparingLong((Snapshot row) -> row.totalDurationNano).reversed());
    return rows;
  }
}
