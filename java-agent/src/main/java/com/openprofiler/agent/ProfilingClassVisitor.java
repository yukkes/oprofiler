package com.openprofiler.agent;

import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;

public class ProfilingClassVisitor extends ClassVisitor {
  private final String className;
  private final boolean recordMethods;
  private final boolean traceAllocationMethods;

  public ProfilingClassVisitor(
      ClassVisitor cv, String className, boolean recordMethods, boolean traceAllocationMethods) {
    super(Opcodes.ASM9, cv);
    this.className = className;
    this.recordMethods = recordMethods;
    this.traceAllocationMethods = traceAllocationMethods;
  }

  @Override
  public MethodVisitor visitMethod(
      int access, String name, String descriptor, String signature, String[] exceptions) {
    MethodVisitor mv = super.visitMethod(access, name, descriptor, signature, exceptions);
    if (mv != null && !name.equals("<clinit>")) {
      return new ProfilingMethodVisitor(
          mv, className, name, descriptor, access, recordMethods, traceAllocationMethods);
    }
    return mv;
  }
}
