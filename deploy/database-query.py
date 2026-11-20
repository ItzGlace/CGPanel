#!/usr/bin/env python3
"""Database Studio executor. Input credentials only on stdin; never use a privileged SQL role.
Single statements, 15-second engine timeout, at most 500 returned rows / 1 MiB JSON.
"""
import base64,hashlib,hmac,json,re,sys,resource,signal,sqlparse
resource.setrlimit(resource.RLIMIT_AS,(512*1024**2,512*1024**2))
signal.alarm(25)

def connect(value):
    name=value['database']
    if not re.fullmatch(r'cgp_[a-f0-9]{20}',name):raise ValueError('Invalid database')
    if value['engine']=='mysql':
        import pymysql
        connection=pymysql.connect(unix_socket='/run/mysqld/mysqld.sock',user=name,password=value['password'],database=name,connect_timeout=5,read_timeout=20,write_timeout=20,autocommit=False,local_infile=False)
        cursor=connection.cursor(pymysql.cursors.SSCursor)
        cursor.execute('SET SESSION max_statement_time=15')
    elif value['engine']=='postgresql':
        import psycopg2
        connection=psycopg2.connect(host='/var/run/postgresql',user=name,password=value['password'],dbname=name,connect_timeout=5,options='-c statement_timeout=15000 -c lock_timeout=5000')
        cursor=connection.cursor()
    else:raise ValueError('Unknown engine')
    return connection,cursor

def identifier(value,engine):
    if not isinstance(value,str) or not value or len(value)>128 or '\0' in value:raise ValueError('Invalid table or column name')
    quote='`' if engine=='mysql' else '"'
    return quote+value.replace(quote,quote*2)+quote

def catalog(cursor,engine):
    if engine=='mysql':
        cursor.execute("SELECT table_schema,table_name,table_type,coalesce(table_rows,0),coalesce(data_length,0)+coalesce(index_length,0) FROM information_schema.tables WHERE table_schema=DATABASE() ORDER BY table_name")
    else:
        cursor.execute("SELECT n.nspname,c.relname,CASE WHEN c.relkind='m' THEN 'MATERIALIZED VIEW' WHEN c.relkind='v' THEN 'VIEW' ELSE 'BASE TABLE' END,greatest(c.reltuples,0)::bigint,pg_total_relation_size(c.oid) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind IN ('r','p','v','m') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp_%' AND has_table_privilege(c.oid,'SELECT') ORDER BY n.nspname,c.relname")
    return [{'schema':r[0],'name':r[1],'type':r[2],'estimated_rows':int(r[3] or 0),'bytes':int(r[4] or 0)} for r in cursor.fetchall()]

def columns(cursor,engine,schema,table):
    if engine=='mysql':
        cursor.execute("SELECT column_name,column_type,is_nullable,column_default,column_key,extra FROM information_schema.columns WHERE table_schema=%s AND table_name=%s ORDER BY ordinal_position",(schema,table))
        return [{'name':r[0],'type':r[1],'nullable':r[2]=='YES','default':None if r[3] is None else str(r[3]),'primary':r[4]=='PRI','generated':'GENERATED' in r[5].upper(),'auto':'auto_increment' in r[5].lower()} for r in cursor.fetchall()]
    cursor.execute("SELECT a.attname,format_type(a.atttypid,a.atttypmod),NOT a.attnotnull,pg_get_expr(d.adbin,d.adrelid),EXISTS(SELECT 1 FROM pg_index i WHERE i.indrelid=c.oid AND i.indisprimary AND a.attnum=ANY(i.indkey)),a.attgenerated<>'',a.attidentity<>'' FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum WHERE n.nspname=%s AND c.relname=%s AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum",(schema,table))
    return [{'name':r[0],'type':r[1],'nullable':r[2],'default':r[3],'primary':r[4],'generated':r[5],'auto':r[6] or bool(r[3] and r[3].startswith('nextval('))} for r in cursor.fetchall()]

def normalized(row):
    return [None if v is None else {'binary':True,'base64':base64.b64encode(bytes(v)).decode()} if isinstance(v,(bytes,bytearray,memoryview)) else json.dumps(v,sort_keys=True,default=str) if isinstance(v,(dict,list)) else str(v) for v in row]

def row_token(value,schema,table,row):
    body=json.dumps([schema,table,normalized(row)],sort_keys=True,separators=(',',':')).encode()
    return hmac.new(value['password'].encode(),body,hashlib.sha256).hexdigest()

def type_sql(value,engine):
    types={'integer':'INTEGER','bigint':'BIGINT','text':'TEXT','varchar':'VARCHAR(255)','boolean':'BOOLEAN','decimal':'DECIMAL(18,6)','timestamp':'TIMESTAMP','date':'DATE','json':'JSON' if engine=='mysql' else 'JSONB','binary':'LONGBLOB' if engine=='mysql' else 'BYTEA'}
    if value not in types:raise ValueError('Choose a supported column type')
    return types[value]

def studio(value,connection,cursor):
    engine=value['engine'];action=value['action']
    if action in ('catalog','size'):
        if engine=='mysql':cursor.execute('SELECT coalesce(sum(data_length+index_length),0),count(*) FROM information_schema.tables WHERE table_schema=DATABASE()')
        else:cursor.execute("SELECT pg_database_size(current_database()),(SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind IN ('r','p','v','m') AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_%')")
        usage=cursor.fetchone();result={'database':value['database'],'engine':engine,'bytes':int(usage[0]),'table_count':int(usage[1]),'estimated':engine=='mysql'}
        if action=='catalog':result['tables']=catalog(cursor,engine)
        return result
    tables=catalog(cursor,engine)
    schema=value.get('schema') or (value['database'] if engine=='mysql' else 'public');table=value.get('table','')
    if engine=='mysql' and schema!=value['database']:raise ValueError('Choose this database schema')
    if engine=='postgresql':
        cursor.execute("SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN ('pg_catalog','information_schema') AND schema_name NOT LIKE 'pg_%'")
        if schema not in [r[0] for r in cursor.fetchall()]:raise ValueError('Choose an accessible application schema')
    qualified=identifier(schema,engine)+'.'+identifier(table,engine)
    selected=next((t for t in tables if t['schema']==schema and t['name']==table),None)
    if action=='create_table':
        if selected:raise ValueError('Table already exists')
        definition=value.get('columns',[])
        if not isinstance(definition,list) or not 1<=len(definition)<=64:raise ValueError('Provide 1–64 columns')
        names=[c.get('name') for c in definition]
        if len(set(names))!=len(names):raise ValueError('Column names must be unique')
        pieces=[];primary=[]
        for column in definition:
            part=identifier(column['name'],engine)+' '+type_sql(column.get('type'),engine)
            if column.get('auto'):
                if column.get('type') not in ('integer','bigint') or not column.get('primary'):raise ValueError('Automatic IDs require an integer primary key')
                part+=' AUTO_INCREMENT' if engine=='mysql' else ' GENERATED BY DEFAULT AS IDENTITY'
            part+=' NOT NULL' if column.get('primary') or not column.get('nullable',True) else ' NULL'
            if column.get('primary'):primary.append(identifier(column['name'],engine))
            pieces.append(part)
        if primary:pieces.append('PRIMARY KEY ('+','.join(primary)+')')
        cursor.execute('CREATE TABLE '+qualified+' ('+','.join(pieces)+')');return {'changed':True}
    if not selected:raise ValueError('Table not found in this database')
    info=columns(cursor,engine,schema,table);primary=[c['name'] for c in info if c['primary']]
    by_name={c['name']:c for c in info}
    if action=='table':
        limit=value.get('limit',50);offset=value.get('offset',0)
        if type(limit)!=int or not 1<=limit<=100 or type(offset)!=int or not 0<=offset<=10000000:raise ValueError('Invalid page range')
        sort=value.get('sort') or (primary[0] if primary else info[0]['name'] if info else None)
        if sort is not None and sort not in by_name:raise ValueError('Choose a valid sort column')
        direction='DESC' if value.get('direction')=='desc' else 'ASC'
        order=([sort] if sort else [])+[name for name in primary if name!=sort]
        cursor.execute('SELECT * FROM '+qualified+(' ORDER BY '+','.join(identifier(name,engine)+' '+direction for name in order) if order else '')+' LIMIT %s OFFSET %s',(limit+1,offset))
        fetched=cursor.fetchmany(limit+1);more=len(fetched)>limit;rows=[];size=0
        for row in fetched[:limit]:
            cells=normalized(row);entry={'values':cells,'token':row_token(value,schema,table,row)}
            for cell in cells:
                if isinstance(cell,dict) and cell.get('binary'):cell['bytes']=len(base64.b64decode(cell['base64']));cell['base64']=cell['base64'][:88]
            size+=len(json.dumps(entry))
            if size>1048576:more=True;break
            rows.append(entry)
        return {'table':selected,'columns':info,'primary_key':primary,'rows':rows,'offset':offset,'limit':limit,'has_more':more and bool(rows),'editable':bool(primary) and selected['type']=='BASE TABLE','bytes_limited':len(rows)<min(len(fetched),limit)}
    if selected['type']!='BASE TABLE' and action not in ('drop_table',):raise ValueError('Views are read-only in the table editor')
    if action=='drop_table':
        if value.get('confirm')!=table:raise ValueError('Type the table name to confirm deletion')
        cursor.execute(('DROP '+selected['type']+' ' if selected['type'] in ('VIEW','MATERIALIZED VIEW') else 'DROP TABLE ')+qualified);return {'changed':True}
    if action=='truncate':
        if value.get('confirm')!=table:raise ValueError('Type the table name to confirm emptying it')
        cursor.execute('TRUNCATE TABLE '+qualified);return {'changed':True}
    if action=='alter_table':
        change=value.get('change');name=value.get('column')
        if change=='add':
            if name in by_name:raise ValueError('Column already exists')
            cursor.execute('ALTER TABLE '+qualified+' ADD COLUMN '+identifier(name,engine)+' '+type_sql(value.get('type'),engine)+' NULL')
        elif change=='rename':
            if name not in by_name:raise ValueError('Column not found')
            cursor.execute('ALTER TABLE '+qualified+' RENAME COLUMN '+identifier(name,engine)+' TO '+identifier(value.get('new_name'),engine))
        elif change=='drop':
            if name not in by_name or value.get('confirm')!=name:raise ValueError('Type the column name to confirm deletion')
            cursor.execute('ALTER TABLE '+qualified+' DROP COLUMN '+identifier(name,engine))
        else:raise ValueError('Choose a column operation')
        return {'changed':True}
    fields=value.get('values',{})
    if not isinstance(fields,dict) or any(name not in by_name or by_name[name]['generated'] for name in fields):raise ValueError('Choose writable columns')
    if any(not (v is None or isinstance(v,(str,int,float,bool))) for v in fields.values()):raise ValueError('Cell values must be text, numbers or NULL')
    if action=='insert':
        if fields:cursor.execute('INSERT INTO '+qualified+' ('+','.join(identifier(name,engine) for name in fields)+') VALUES ('+','.join('%s' for _ in fields)+')',list(fields.values()))
        else:cursor.execute('INSERT INTO '+qualified+(' () VALUES ()' if engine=='mysql' else ' DEFAULT VALUES'))
        return {'changed':True,'affected':cursor.rowcount}
    if action not in ('update','delete') or not primary:raise ValueError('A primary key is required to edit or delete a row safely')
    key=value.get('key',{})
    if not isinstance(key,dict) or set(key)!=set(primary) or any(v is None or not isinstance(v,(str,int,float,bool)) for v in key.values()):raise ValueError('Provide the complete primary key')
    where=' AND '.join(identifier(name,engine)+'=%s' for name in primary);params=[key[name] for name in primary]
    cursor.execute('SELECT * FROM '+qualified+' WHERE '+where+' FOR UPDATE',params);row=cursor.fetchone()
    if row is None:raise ValueError('Row no longer exists; refresh the table')
    if not hmac.compare_digest(str(value.get('token','')),row_token(value,schema,table,row)):raise ValueError('Row changed since it was loaded; refresh before saving')
    if action=='delete':cursor.execute('DELETE FROM '+qualified+' WHERE '+where,params)
    else:
        if not fields:raise ValueError('No values to update')
        cursor.execute('UPDATE '+qualified+' SET '+','.join(identifier(name,engine)+'=%s' for name in fields)+' WHERE '+where,list(fields.values())+params)
    return {'changed':True,'affected':cursor.rowcount}

def execute(value):
    connection,cursor=connect(value)
    try:
        if value.get('action','sql')!='sql':
            result=studio(value,connection,cursor);connection.commit();return result
        sql=value.get('sql','')
        if not sql.strip() or len(sql.encode())>262144:raise ValueError('Invalid SQL size')
        if len(sqlparse.split(sql))!=1:raise ValueError('Execute one statement at a time')
        cursor.execute(sql)
        columns=[v[0] for v in cursor.description] if cursor.description else []
        rows=[];truncated=False;size=0
        if columns:
            for row in cursor:
                normalized=[None if v is None else str(v) for v in row]
                encoded=json.dumps(normalized)
                if len(rows)>=500 or size+len(encoded)>1048576:truncated=True;break
                rows.append(normalized);size+=len(encoded)
        result={'columns':columns,'rows':rows,'affected':len(rows) if columns else max(0,cursor.rowcount),'truncated':truncated}
        # Truncated statements are rolled back (e.g. large UPDATE RETURNING).
        if truncated:connection.rollback()
        else:connection.commit()
        return result
    finally:connection.close()

if __name__=='__main__':
    try:print(json.dumps(execute(json.load(sys.stdin))))
    except MemoryError:
        print('Result exceeds the safe memory budget; select fewer or smaller columns in SQL.',file=sys.stderr);sys.exit(1)
    except Exception as error:
        # Database messages can include SQL; caller receives a concise message, not credentials.
        print(str(error)[:800],file=sys.stderr);sys.exit(1)
