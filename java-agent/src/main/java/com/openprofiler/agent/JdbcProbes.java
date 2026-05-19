package com.openprofiler.agent;

final class JdbcProbes {
  static final int STATEMENT_EXECUTE_QUERY = 0;
  static final int STATEMENT_CLOSE = 1;
  static final int PREPARED_STATEMENT_EXECUTE_QUERY = 2;
  static final int PREPARED_STATEMENT_EXECUTE = 3;
  static final int PREPARED_STATEMENT_EXECUTE_BATCH = 4;
  static final int PREPARED_STATEMENT_EXECUTE_UPDATE = 5;
  static final int PREPARED_STATEMENT_SET_STRING = 6;
  static final int PREPARED_STATEMENT_ADD_BATCH = 7;
  static final int CONNECTION_PREPARE_STATEMENT = 8;
  static final int CONNECTION_CREATE_STATEMENT = 9;
  static final int CONNECTION_CLOSE = 10;
  static final int DATA_SOURCE_GET_CONNECTION = 11;
  static final int COUNT = 12;

  static final String[] CLASS_NAMES = {
    "java.sql.Statement",
    "java.sql.Statement",
    "java.sql.PreparedStatement",
    "java.sql.PreparedStatement",
    "java.sql.PreparedStatement",
    "java.sql.PreparedStatement",
    "java.sql.PreparedStatement",
    "java.sql.PreparedStatement",
    "java.sql.Connection",
    "java.sql.Connection",
    "java.sql.Connection",
    "javax.sql.DataSource"
  };

  static final String[] METHOD_NAMES = {
    "executeQuery",
    "close",
    "executeQuery",
    "execute",
    "executeBatch",
    "executeUpdate",
    "setString",
    "addBatch",
    "prepareStatement",
    "createStatement",
    "close",
    "getConnection"
  };

  static final String[] DESCRIPTORS = {
    "(Ljava/lang/String;)Ljava/sql/ResultSet;",
    "()V",
    "()Ljava/sql/ResultSet;",
    "()Z",
    "()[I",
    "()I",
    "(ILjava/lang/String;)V",
    "()V",
    "(Ljava/lang/String;)Ljava/sql/PreparedStatement;",
    "()Ljava/sql/Statement;",
    "()V",
    "()Ljava/sql/Connection;"
  };

  private JdbcProbes() {}

  static String key(int probeId) {
    return CLASS_NAMES[probeId] + "." + METHOD_NAMES[probeId] + DESCRIPTORS[probeId];
  }
}
