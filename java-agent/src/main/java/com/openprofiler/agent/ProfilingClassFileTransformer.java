package com.openprofiler.agent;

import java.lang.instrument.ClassFileTransformer;
import java.security.ProtectionDomain;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.ClassWriter;

public class ProfilingClassFileTransformer implements ClassFileTransformer {
  private static final Set<String> transformedClasses = ConcurrentHashMap.newKeySet();

  @Override
  public byte[] transform(
      ClassLoader loader,
      String className,
      Class<?> classBeingRedefined,
      ProtectionDomain protectionDomain,
      byte[] classfileBuffer) {
    if (!shouldInspect(className, loader)) {
      return classfileBuffer;
    }

    try {
      if (!JavaAgent.isRecording()) {
        return classfileBuffer;
      }
      boolean recordMethods = shouldRecordMethods(className, loader);
      boolean traceAllocationMethods = isAllocationTraceClass(className);
      if (!recordMethods && !traceAllocationMethods) {
        return classfileBuffer;
      }
      ClassReader cr = new ClassReader(classfileBuffer);
      ClassWriter cw = new ClassWriter(cr, ClassWriter.COMPUTE_FRAMES | ClassWriter.COMPUTE_MAXS);
      ClassVisitor cv =
          new ProfilingClassVisitor(cw, className, recordMethods, traceAllocationMethods);
      cr.accept(cv, ClassReader.EXPAND_FRAMES);
      transformedClasses.add(className);
      return cw.toByteArray();
    } catch (Exception e) {
      System.err.println(
          "[java-agent] Error transforming class " + className + ": " + e.getMessage());
      return classfileBuffer;
    }
  }

  public static boolean shouldRetransformClass(Class<?> cls) {
    if (cls == null || !JavaAgent.isModifiable(cls)) {
      return false;
    }
    String className = cls.getName().replace('.', '/');
    if (!shouldInspect(className, cls.getClassLoader())) {
      return false;
    }
    if (transformedClasses.contains(className)) {
      return false;
    }
    return shouldRecordMethods(className, cls.getClassLoader())
        || isAllocationTraceClass(className);
  }

  public static boolean shouldRestoreClass(Class<?> cls) {
    if (cls == null || !JavaAgent.isModifiable(cls)) {
      return false;
    }
    String className = cls.getName().replace('.', '/');
    return transformedClasses.contains(className);
  }

  public static void markRestored(Class<?> cls) {
    if (cls == null) {
      return;
    }
    transformedClasses.remove(cls.getName().replace('.', '/'));
  }

  private static boolean shouldInspect(String className, ClassLoader loader) {
    if (className == null || className.equals("jdbc/CustomFunction")) {
      return false;
    }
    if (className.startsWith("com/openprofiler/agent/")
        || className.startsWith("com/openprofiler/protocol/")
        || className.startsWith("com/google/protobuf/")
        || className.startsWith("org/objectweb/asm/")) {
      return false;
    }
    if (isAllocationTraceClass(className)) {
      return true;
    }
    return shouldRecordMethods(className, loader);
  }

  private static boolean shouldRecordMethods(String className, ClassLoader loader) {
    if ((loader == null && !className.startsWith("com/openprofiler/benchmark/"))
        || isJdkClass(className)
        || isJdbcDriverClass(className)
        || isCommonLibraryClass(className)) {
      return false;
    }
    if (matchesAnyPrefix(className, System.getProperty("openprofiler.instrument.excludes", ""))) {
      return false;
    }
    String includes = System.getProperty("openprofiler.instrument.includes", "").trim();
    return includes.isEmpty() || matchesAnyPrefix(className, includes);
  }

  private static boolean matchesAnyPrefix(String className, String patterns) {
    if (patterns == null || patterns.isBlank()) {
      return false;
    }
    for (String pattern : patterns.split("[|,]")) {
      String prefix = pattern.trim().replace('.', '/');
      if (prefix.endsWith("*")) {
        prefix = prefix.substring(0, prefix.length() - 1);
      }
      if (!prefix.isEmpty() && className.startsWith(prefix)) {
        return true;
      }
    }
    return false;
  }

  private static boolean isJdkClass(String className) {
    return className.startsWith("java/")
        || className.startsWith("javax/")
        || className.startsWith("sun/")
        || className.startsWith("jdk/")
        || className.startsWith("com/sun/");
  }

  private static boolean isCommonLibraryClass(String className) {
    return className.startsWith("org/springframework/")
        || className.startsWith("org/apache/")
        || className.startsWith("org/objectweb/")
        || className.startsWith("org/slf4j/")
        || className.startsWith("ch/qos/logback/")
        || className.startsWith("com/fasterxml/")
        || className.startsWith("com/google/")
        || className.startsWith("io/netty/")
        || className.startsWith("io/micrometer/")
        || className.startsWith("reactor/")
        || className.startsWith("kotlin/")
        || className.startsWith("scala/");
  }

  private static boolean isAllocationTraceClass(String className) {
    return className.equals("java/net/URL")
        || className.equals("java/net/HttpURLConnection")
        || className.equals("sun/net/www/protocol/http/HttpURLConnection")
        || className.equals("java/io/InputStreamReader")
        || className.equals("java/io/BufferedReader")
        || className.equals("java/io/OutputStream")
        || className.endsWith("OutputStream")
        || className.equals("sun/net/httpserver/ExchangeImpl")
        || className.equals("com/sun/net/httpserver/HttpExchange")
        || className.equals("java/util/concurrent/ThreadPoolExecutor")
        || className.equals("java/util/concurrent/ThreadPoolExecutor$Worker")
        || className.startsWith("java/lang/String")
        || className.startsWith("java/lang/AbstractStringBuilder")
        || className.equals("java/rmi/registry/Registry")
        || className.equals("javax/persistence/EntityManager")
        || className.equals("javax/persistence/TypedQuery");
  }

  private static boolean isJdbcDriverClass(String className) {
    return className.startsWith("org/hsqldb/")
        || className.startsWith("org/postgresql/")
        || className.startsWith("com/mysql/")
        || className.startsWith("com/microsoft/sqlserver/")
        || className.startsWith("oracle/jdbc/")
        || className.startsWith("org/mariadb/jdbc/")
        || className.startsWith("com/ibm/db2/");
  }
}
