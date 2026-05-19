package com.openprofiler.agent;

import java.util.ArrayDeque;
import java.util.Deque;
import org.objectweb.asm.Label;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;
import org.objectweb.asm.commons.AdviceAdapter;

public class ProfilingMethodVisitor extends AdviceAdapter {
  private final String className;
  private final String methodName;
  private final String methodDescriptor;
  private final int methodId;
  private final boolean recordMethods;
  private final boolean traceAllocationMethods;
  private final boolean allocationBoundaryMethod;
  private final Deque<String> pendingNewTypes = new ArrayDeque<>();

  protected ProfilingMethodVisitor(
      MethodVisitor mv,
      String className,
      String methodName,
      String methodDescriptor,
      int access,
      boolean recordMethods,
      boolean traceAllocationMethods) {
    super(Opcodes.ASM9, mv, access, methodName, methodDescriptor);
    this.className = className;
    this.methodName = methodName;
    this.methodDescriptor = methodDescriptor;
    this.recordMethods = recordMethods;
    this.traceAllocationMethods = traceAllocationMethods;
    this.allocationBoundaryMethod =
        isAllocationBoundaryMethod(className, methodName, methodDescriptor);
    this.methodId =
        (recordMethods || (traceAllocationMethods && allocationBoundaryMethod))
            ? Profiler.registerMethod(className, methodName, methodDescriptor)
            : -1;
  }

  @Override
  protected void onMethodEnter() {
    if (recordMethods) {
      Label skip = new Label();
      mv.visitMethodInsn(
          INVOKESTATIC, "com/openprofiler/agent/JavaAgent", "isRecording", "()Z", false);
      mv.visitJumpInsn(IFEQ, skip);
      mv.visitLdcInsn(methodId);
      mv.visitMethodInsn(
          INVOKESTATIC, "com/openprofiler/agent/Profiler", "recordMethodEnter", "(I)V", false);
      mv.visitLabel(skip);
    } else if (traceAllocationMethods && allocationBoundaryMethod) {
      Label skip = new Label();
      mv.visitMethodInsn(
          INVOKESTATIC, "com/openprofiler/agent/JavaAgent", "isRecording", "()Z", false);
      mv.visitJumpInsn(IFEQ, skip);
      mv.visitLdcInsn(methodId);
      mv.visitLdcInsn(allocationBoundaryMethod);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/Profiler",
          "recordAllocationTraceEnter",
          "(IZ)V",
          false);
      mv.visitLabel(skip);
    }
  }

  @Override
  protected void onMethodExit(int opcode) {
    if (recordMethods) {
      Label skip = new Label();
      mv.visitMethodInsn(
          INVOKESTATIC, "com/openprofiler/agent/JavaAgent", "isRecording", "()Z", false);
      mv.visitJumpInsn(IFEQ, skip);
      mv.visitLdcInsn(methodId);
      mv.visitMethodInsn(
          INVOKESTATIC, "com/openprofiler/agent/Profiler", "recordMethodExit", "(I)V", false);
      mv.visitLabel(skip);
    } else if (traceAllocationMethods && allocationBoundaryMethod) {
      Label skip = new Label();
      mv.visitMethodInsn(
          INVOKESTATIC, "com/openprofiler/agent/JavaAgent", "isRecording", "()Z", false);
      mv.visitJumpInsn(IFEQ, skip);
      mv.visitLdcInsn(methodId);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/Profiler",
          "recordAllocationTraceExit",
          "(I)V",
          false);
      mv.visitLabel(skip);
    }
  }

  @Override
  public void visitTypeInsn(int opcode, String type) {
    if (opcode == Opcodes.NEW) {
      pendingNewTypes.push(type);
    }
    super.visitTypeInsn(opcode, type);
    if (opcode == Opcodes.ANEWARRAY) {
      recordAllocation("[L" + type + ";");
    }
  }

  @Override
  public void visitMethodInsn(
      int opcode, String owner, String name, String descriptor, boolean isInterface) {
    if (opcode == INVOKESPECIAL && name.equals("<init>") && !pendingNewTypes.isEmpty()) {
      super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
      String allocatedType = pendingNewTypes.pop();
      if (owner.equals(allocatedType)) {
        recordAllocation(allocatedType);
      }
      return;
    }
    if (isJdbcOwner(owner) && isDirectSqlCall(name, descriptor)) {
      int sqlLocal = newLocal(Type.getType(String.class));
      storeLocal(sqlLocal);
      loadLocal(sqlLocal);
      enterSql();
      loadLocal(sqlLocal);
      super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
      exitSqlPreservingReturn(descriptor, sqlLocal);
      return;
    }
    if (isJdbcOwner(owner) && isPrepareSqlCall(name, descriptor)) {
      int sqlLocal = newLocal(Type.getType(String.class));
      storeLocal(sqlLocal);
      loadLocal(sqlLocal);
      enterSql();
      loadLocal(sqlLocal);
      super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
      Type returnType = Type.getReturnType(descriptor);
      int statementLocal = newLocal(returnType);
      storeLocal(statementLocal);
      loadLocal(statementLocal);
      loadLocal(sqlLocal);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/SqlHotSpots",
          "associatePreparedStatement",
          "(Ljava/lang/Object;Ljava/lang/String;)V",
          false);
      exitSql(sqlLocal);
      loadLocal(statementLocal);
      return;
    }
    if (isJdbcOwner(owner) && isPreparedSqlExecution(name, descriptor)) {
      mv.visitInsn(DUP);
      int statementLocal = newLocal(Type.getType(Object.class));
      storeLocal(statementLocal);
      loadLocal(statementLocal);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/SqlHotSpots",
          "enterPrepared",
          "(Ljava/lang/Object;)V",
          false);
      super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
      exitPreparedSqlPreservingReturn(descriptor);
      return;
    }
    if (JavaAgent.isJdbcProbeEnabled() && isApiHotSpot(owner)) {
      String apiClassName = owner.replace('/', '.');
      mv.visitLdcInsn(apiClassName);
      mv.visitLdcInsn(name);
      mv.visitLdcInsn(descriptor);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/Profiler",
          "recordJdbcEnter",
          "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
          false);
      super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
      mv.visitLdcInsn(apiClassName);
      mv.visitLdcInsn(name);
      mv.visitLdcInsn(descriptor);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/Profiler",
          "recordJdbcExit",
          "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
          false);
      return;
    }
    int probeId = JavaAgent.isJdbcProbeEnabled() ? jdbcProbeId(owner, name, descriptor) : -1;
    if (probeId >= 0) {
      String mode = JavaAgent.getJdbcProbeMode();
      if ("native".equals(mode) && JavaAgent.isNativeTimingAvailable()) {
        mv.visitLdcInsn(probeId);
        mv.visitMethodInsn(
            INVOKESTATIC, "com/openprofiler/agent/Agent", "recordJdbcProbeEnter", "(I)V", false);
        super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
        mv.visitLdcInsn(probeId);
        mv.visitMethodInsn(
            INVOKESTATIC, "com/openprofiler/agent/Agent", "recordJdbcProbeExit", "(I)V", false);
        return;
      }
      if ("id".equals(mode)) {
        mv.visitLdcInsn(probeId);
        mv.visitMethodInsn(
            INVOKESTATIC, "com/openprofiler/agent/Profiler", "recordJdbcProbeEnter", "(I)V", false);
        super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
        mv.visitLdcInsn(probeId);
        mv.visitMethodInsn(
            INVOKESTATIC, "com/openprofiler/agent/Profiler", "recordJdbcProbeExit", "(I)V", false);
        return;
      }
      String className = owner.replace('/', '.');
      mv.visitLdcInsn(className);
      mv.visitLdcInsn(name);
      mv.visitLdcInsn(descriptor);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/Profiler",
          "recordJdbcEnter",
          "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
          false);
      super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
      mv.visitLdcInsn(className);
      mv.visitLdcInsn(name);
      mv.visitLdcInsn(descriptor);
      mv.visitMethodInsn(
          INVOKESTATIC,
          "com/openprofiler/agent/Profiler",
          "recordJdbcExit",
          "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
          false);
      return;
    }
    super.visitMethodInsn(opcode, owner, name, descriptor, isInterface);
  }

  @Override
  public void visitIntInsn(int opcode, int operand) {
    super.visitIntInsn(opcode, operand);
    if (opcode == NEWARRAY) {
      recordAllocation(primitiveArrayType(operand));
    }
  }

  @Override
  public void visitMultiANewArrayInsn(String descriptor, int numDimensions) {
    super.visitMultiANewArrayInsn(descriptor, numDimensions);
    recordAllocation(descriptor);
  }

  private void recordAllocation(String allocatedType) {
    mv.visitInsn(DUP);
    mv.visitLdcInsn(allocatedType);
    mv.visitLdcInsn(methodId);
    mv.visitMethodInsn(
        INVOKESTATIC,
        "com/openprofiler/agent/Profiler",
        "objectAllocatedAt",
        "(Ljava/lang/Object;Ljava/lang/String;I)V",
        false);
  }

  private static String primitiveArrayType(int operand) {
    switch (operand) {
      case T_BOOLEAN:
        return "[Z";
      case T_CHAR:
        return "[C";
      case T_FLOAT:
        return "[F";
      case T_DOUBLE:
        return "[D";
      case T_BYTE:
        return "[B";
      case T_SHORT:
        return "[S";
      case T_INT:
        return "[I";
      case T_LONG:
        return "[J";
      default:
        return "[?";
    }
  }

  private static boolean isApiHotSpot(String owner) {
    return owner.startsWith("java/sql/")
        || owner.startsWith("javax/sql/")
        || owner.startsWith("javax/persistence/")
        || owner.startsWith("javax/naming/");
  }

  private static boolean isAllocationBoundaryMethod(
      String className, String methodName, String descriptor) {
    return (className.equals("java/net/HttpURLConnection") && methodName.equals("getResponseCode"))
        || (className.equals("sun/net/www/protocol/http/HttpURLConnection")
            && methodName.equals("getResponseCode"))
        || (className.equals("java/net/URL") && methodName.equals("openConnection"))
        || (className.equals("java/io/InputStreamReader") && methodName.equals("<init>"))
        || (className.equals("java/io/BufferedReader") && methodName.equals("readLine"))
        || (className.equals("java/io/OutputStream") && methodName.equals("close"))
        || (className.endsWith("OutputStream") && methodName.equals("close"))
        || (className.equals("java/lang/String") && methodName.equals("split"))
        || (className.equals("java/lang/String") && methodName.equals("getBytes"))
        || (className.equals("java/rmi/registry/Registry") && methodName.equals("lookup"))
        || (className.equals("com/sun/net/httpserver/HttpExchange")
            && methodName.equals("sendResponseHeaders"))
        || (className.equals("sun/net/httpserver/ExchangeImpl")
            && methodName.equals("sendResponseHeaders"))
        || (className.equals("java/util/concurrent/ThreadPoolExecutor$Worker")
            && methodName.equals("run"))
        || (className.equals("java/util/concurrent/ThreadPoolExecutor")
            && methodName.equals("runWorker"))
        || (className.equals("javax/persistence/EntityManager") && methodName.equals("flush"))
        || (className.equals("javax/persistence/TypedQuery") && methodName.equals("getResultList"));
  }

  private static boolean isDirectSqlCall(String name, String descriptor) {
    return descriptor.startsWith("(Ljava/lang/String;")
        && (name.equals("execute")
            || name.equals("executeQuery")
            || name.equals("executeUpdate")
            || name.equals("executeLargeUpdate")
            || name.equals("addBatch"));
  }

  private static boolean isPrepareSqlCall(String name, String descriptor) {
    return (name.equals("prepareStatement") || name.equals("prepareCall"))
        && descriptor.startsWith("(Ljava/lang/String;")
        && descriptor.endsWith(";");
  }

  private static boolean isPreparedSqlExecution(String name, String descriptor) {
    return descriptor.startsWith("()")
        && (name.equals("execute")
            || name.equals("executeQuery")
            || name.equals("executeUpdate")
            || name.equals("executeLargeUpdate")
            || name.equals("executeBatch")
            || name.equals("executeLargeBatch"));
  }

  private static boolean isJdbcOwner(String owner) {
    return owner.startsWith("java/sql/")
        || owner.startsWith("javax/sql/")
        || owner.startsWith("org/hsqldb/")
        || owner.startsWith("org/postgresql/")
        || owner.startsWith("com/mysql/")
        || owner.startsWith("com/microsoft/sqlserver/")
        || owner.startsWith("oracle/jdbc/")
        || owner.startsWith("org/mariadb/jdbc/")
        || owner.startsWith("com/ibm/db2/");
  }

  private void enterSql() {
    mv.visitMethodInsn(
        INVOKESTATIC,
        "com/openprofiler/agent/Profiler",
        "recordSqlEnter",
        "(Ljava/lang/String;)V",
        false);
  }

  private void exitSql(int sqlLocal) {
    loadLocal(sqlLocal);
    mv.visitMethodInsn(
        INVOKESTATIC,
        "com/openprofiler/agent/Profiler",
        "recordSqlExit",
        "(Ljava/lang/String;)V",
        false);
  }

  private void exitPreparedSql() {
    mv.visitMethodInsn(INVOKESTATIC, "com/openprofiler/agent/SqlHotSpots", "exit", "()V", false);
  }

  private void exitPreparedSqlPreservingReturn(String descriptor) {
    Type returnType = Type.getReturnType(descriptor);
    if (returnType.getSort() == Type.VOID) {
      exitPreparedSql();
      return;
    }
    int returnLocal = newLocal(returnType);
    storeLocal(returnLocal);
    exitPreparedSql();
    loadLocal(returnLocal);
  }

  private void exitSqlPreservingReturn(String descriptor, int sqlLocal) {
    Type returnType = Type.getReturnType(descriptor);
    if (returnType.getSort() == Type.VOID) {
      exitSql(sqlLocal);
      return;
    }
    int returnLocal = newLocal(returnType);
    storeLocal(returnLocal);
    exitSql(sqlLocal);
    loadLocal(returnLocal);
  }

  private static int jdbcProbeId(String owner, String name, String descriptor) {
    if (owner.equals("java/sql/Statement")) {
      if (name.equals("executeQuery")
          && descriptor.equals("(Ljava/lang/String;)Ljava/sql/ResultSet;")) {
        return JdbcProbes.STATEMENT_EXECUTE_QUERY;
      }
      if (name.equals("close") && descriptor.equals("()V")) {
        return JdbcProbes.STATEMENT_CLOSE;
      }
    }
    if (owner.equals("java/sql/PreparedStatement")) {
      if (name.equals("executeQuery") && descriptor.equals("()Ljava/sql/ResultSet;")) {
        return JdbcProbes.PREPARED_STATEMENT_EXECUTE_QUERY;
      }
      if (name.equals("execute") && descriptor.equals("()Z")) {
        return JdbcProbes.PREPARED_STATEMENT_EXECUTE;
      }
      if (name.equals("executeBatch") && descriptor.equals("()[I")) {
        return JdbcProbes.PREPARED_STATEMENT_EXECUTE_BATCH;
      }
      if (name.equals("executeUpdate") && descriptor.equals("()I")) {
        return JdbcProbes.PREPARED_STATEMENT_EXECUTE_UPDATE;
      }
      if (name.equals("setString") && descriptor.equals("(ILjava/lang/String;)V")) {
        return JdbcProbes.PREPARED_STATEMENT_SET_STRING;
      }
      if (name.equals("addBatch") && descriptor.equals("()V")) {
        return JdbcProbes.PREPARED_STATEMENT_ADD_BATCH;
      }
    }
    if (owner.equals("java/sql/Connection")) {
      if (name.equals("prepareStatement")
          && descriptor.equals("(Ljava/lang/String;)Ljava/sql/PreparedStatement;")) {
        return JdbcProbes.CONNECTION_PREPARE_STATEMENT;
      }
      if (name.equals("createStatement") && descriptor.equals("()Ljava/sql/Statement;")) {
        return JdbcProbes.CONNECTION_CREATE_STATEMENT;
      }
      if (name.equals("close") && descriptor.equals("()V")) {
        return JdbcProbes.CONNECTION_CLOSE;
      }
    }
    if (owner.equals("javax/sql/DataSource")) {
      if (name.equals("getConnection") && descriptor.equals("()Ljava/sql/Connection;")) {
        return JdbcProbes.DATA_SOURCE_GET_CONNECTION;
      }
    }
    return -1;
  }
}
