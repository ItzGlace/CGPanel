#!/usr/bin/env python3
"""Database Studio executor. Input credentials only on stdin; never use a privileged SQL role.
Single statements, 15-second engine timeout, at most 500 returned rows / 1 MiB JSON.
"""
import json,re,sys,resource,signal,sqlparse
resource.setrlimit(resource.RLIMIT_AS,(512*1024**2,512*1024**2))
signal.alarm(25)

def execute(value):
    name=value['database']
    if not re.fullmatch(r'cgp_[a-f0-9]{20}',name):raise ValueError('Invalid database')
    sql=value['sql']
    if not sql.strip() or len(sql.encode())>262144:raise ValueError('Invalid SQL size')
    if len(sqlparse.split(sql))!=1:raise ValueError('Execute one statement at a time')
    if value['engine']=='mysql':
        import pymysql
        connection=pymysql.connect(unix_socket='/run/mysqld/mysqld.sock',user=name,password=value['password'],database=name,connect_timeout=5,read_timeout=20,write_timeout=20,autocommit=False,local_infile=False)
        cursor=connection.cursor(pymysql.cursors.SSCursor)
        cursor.execute('SET SESSION max_statement_time=15');cursor.execute(sql)
    elif value['engine']=='postgresql':
        import psycopg2
        connection=psycopg2.connect(host='/var/run/postgresql',user=name,password=value['password'],dbname=name,connect_timeout=5,options='-c statement_timeout=15000 -c lock_timeout=5000')
        cursor=connection.cursor();cursor.execute(sql)
    else:raise ValueError('Unknown engine')
    try:
        columns=[v[0] for v in cursor.description] if cursor.description else []
        rows=[];truncated=False;size=0
        if columns:
            for row in cursor:
                normalized=[None if v is None else str(v) for v in row]
                encoded=json.dumps(normalized)
                if len(rows)>=500 or size+len(encoded)>1048576:truncated=True;break
                rows.append(normalized);size+=len(encoded)
        result={'columns':columns,'rows':rows,'affected':max(0,cursor.rowcount),'truncated':truncated}
        # Truncated statements are rolled back (e.g. large UPDATE RETURNING).
        if truncated:connection.rollback()
        else:connection.commit()
        return result
    finally:connection.close()

if __name__=='__main__':
    try:print(json.dumps(execute(json.load(sys.stdin))))
    except Exception as error:
        # Database messages can include SQL; caller receives a concise message, not credentials.
        print(str(error)[:800],file=sys.stderr);sys.exit(1)
