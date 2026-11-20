export function formatStorage(bytes) {
  const n=Number(bytes);
  if(!Number.isFinite(n)||n<0)return 'Unavailable';
  const units=['B','KB','MB','GB','TB','PB','EB'];
  const index=n>0?Math.min(units.length-1,Math.floor(Math.log(n)/Math.log(1000))):0;
  return (n/1000**index).toLocaleString(undefined,{maximumFractionDigits:index?2:0})+' '+units[index];
}
export function databaseWorkspace({api,esc,modal,toast}) {
  const $=(q,root=document)=>root.querySelector(q);
  const button=(label,extra='')=>`<button type="button" class="studio-button" ${extra}>${label}</button>`;
  const field=(name,label,value='')=>`<label class="studio-field">${esc(label)}<input name="${name}" value="${esc(value)}" required></label>`;
  const types=['integer','bigint','varchar','text','boolean','decimal','timestamp','date','json','binary'];
  const typeSelect=name=>`<select name="${name}" aria-label="Column type">${types.map(t=>`<option>${t}</option>`).join('')}</select>`;
  const size=n=>`<span title="${Number(n).toLocaleString()} bytes">${formatStorage(n)}</span>`;
  const cell=value=>value===null?'<span class="text-slate-400 italic">NULL</span>':typeof value==='object'?`<span class="text-slate-500">Binary · ${value.bytes} bytes</span>`:esc(value);
  const csv=(names,rows)=>{
    const quote=v=>'"'+String(v??'').replace(/^[=+\-@\t\r]/,"'$&").replace(/"/g,'""')+'"';
    const url=URL.createObjectURL(new Blob([[names,...rows].map(row=>row.map(quote).join(',')).join('\r\n')],{type:'text/csv;charset=utf-8'}));
    const link=document.createElement('a');link.href=url;link.download='database-page.csv';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  };
  let generation=0;
  return async function render() {
    const epoch=++generation,databases=await api('/resources/databases');
    if(epoch!==generation)return;
    const root=$('#content');
    if(!databases.length){root.innerHTML='<section class="studio-card"><h2>No databases yet</h2><p>Create a database to open its tables here.</p><a href="#databases">Manage databases →</a></section>';return;}
    let db=databases.find(d=>d.id===sessionStorage.getItem('cgp-studio'))||databases[0],catalog=null,table=null,page=null,tab='browse',offset=0,sort='',direction='asc',requestId=0,previousOffsets=[];
    root.innerHTML=`<div id="studio-workspace" class="studio-layout"><aside class="studio-sidebar studio-card"><h2>Databases</h2><p class="studio-muted">Select a database, then a table.</p><div id="studio-tree"></div></aside><section class="studio-card studio-main"><div class="studio-toolbar"><div><p id="studio-breadcrumb" class="studio-muted"></p><h2 id="studio-heading">Database overview</h2></div><div>${button('Refresh','id="studio-refresh"')}${button('New table','id="studio-create"')}</div></div><div id="studio-notice" role="status" aria-live="polite"></div><nav id="studio-tabs" class="studio-tabs" aria-label="Database workspace">${['browse','structure','insert','sql'].map(t=>button(t==='sql'?'SQL':t[0].toUpperCase()+t.slice(1),`data-studio-tab="${t}"`)).join('')}</nav><div id="studio-body"></div></section></div>`;
    const shell=$('#studio-workspace');const live=()=>epoch===generation&&shell.isConnected;
    const notice=(message,error=false)=>{if(live()){const el=$('#studio-notice');el.textContent=message;el.className=error?'studio-error':'studio-muted';}};
    const call=(body,database=db)=>api('/databases/'+database.id+'/studio','POST',body);
    const target=()=>({schema:table.schema,table:table.name});
    const close=()=>$('#dialog').close();
    function tree(){
      $('#studio-tree').innerHTML=databases.map(item=>`<div class="studio-tree-group">${button(`${esc(item.name)}<small>${esc(item.data.engine)}</small>`,`data-studio-db="${item.id}" aria-current="${item.id===db.id}"`)}${item.id===db.id?`<input id="studio-filter" class="studio-filter" aria-label="Filter tables" placeholder="Find a table…"><div id="studio-table-tree">${(catalog?.tables||[]).map((t,i)=>button(`${t.type.endsWith('VIEW')?'◇':'▦'} ${esc(t.name)}<small>${esc(t.schema)}</small>`,`data-studio-table="${i}" aria-current="${table?.schema===t.schema&&table?.name===t.name}"`)).join('')||'<p class="studio-muted p-3">No tables. Create your first table.</p>'}</div>`:''}</div>`).join('');
      $('#studio-filter')?.addEventListener('input',e=>shell.querySelectorAll('[data-studio-table]').forEach(el=>el.hidden=!el.textContent.toLowerCase().includes(e.target.value.toLowerCase())));
    }
    function heading(){
      $('#studio-breadcrumb').textContent=db.name+(table?' / '+table.schema+' / '+table.name:'');$('#studio-heading').textContent=table?table.name:'Database overview';
      shell.querySelectorAll('[data-studio-tab]').forEach(el=>{el.setAttribute('aria-selected',String(el.dataset.studioTab===tab));el.disabled=!table&&el.dataset.studioTab!=='sql';});
    }
    function busy(message){page=null;$('#studio-body').textContent=message;$('#studio-create').disabled=true;shell.querySelectorAll('[data-studio-tab]').forEach(el=>el.disabled=true);notice(message);}
    async function loadCatalog(select=null){
      const selected=db,id=++requestId;busy('Loading database…');
      try{const result=await call({action:'catalog'},selected);if(!live()||id!==requestId)return;catalog=result;$('#studio-create').disabled=false;table=select?catalog.tables.find(t=>t.schema===select.schema&&t.name===select.table):null;tree();notice('');if(table)await loadTable();else overview();}catch(error){notice(error.message,true);}
    }
    function overview(){heading();$('#studio-body').innerHTML=`<div class="studio-summary"><strong>${catalog.tables.length} tables / views</strong><span>${size(catalog.bytes??catalog.tables.reduce((n,t)=>n+t.bytes,0))} estimated storage</span></div><p class="studio-muted">Choose a table in the sidebar to browse rows, inspect columns or insert data. SQL remains available in its own tab.</p><div class="studio-table-cards">${catalog.tables.map((t,i)=>button(`<strong>${esc(t.name)}</strong><small>${esc(t.schema)} · ${esc(t.type)} · ~${t.estimated_rows} rows · ${size(t.bytes)}</small>`,`data-studio-table="${i}"`)).join('')}</div>`;}
    async function loadTable(){
      if(!table)return overview();const selected=target(),id=++requestId,database=db;heading();busy('Loading table…');
      try{const result=await call({action:'table',...selected,offset,limit:50,sort,direction},database);if(!live()||id!==requestId)return;page=result;$('#studio-create').disabled=false;notice(result.bytes_limited?'This page reached the 1 MiB result limit. Use SQL to select fewer columns.':'');paint();}catch(error){notice(error.message,true);}
    }
    function paint(){
      heading();tree();if(tab==='sql')return sqlView();if(tab==='insert')return rowForm(null,true);if(tab==='structure')return structure();
      $('#studio-body').innerHTML=`<div class="studio-toolbar"><p class="studio-muted">Rows ${page.rows.length?offset+1:0}–${offset+page.rows.length} · ~${table.estimated_rows} estimated total${page.editable?'':' · row editing requires a primary key'}</p><div>${table.type==='BASE TABLE'?button('Empty table','data-studio-empty'):''}${button('Export this page','id="studio-csv"')}${button('Previous','id="studio-prev" '+(offset?'':'disabled'))}${button('Next','id="studio-next" '+(page.has_more?'':'disabled'))}</div></div><div class="studio-scroll"><table class="studio-data"><thead><tr><th>Actions</th>${page.columns.map((c,i)=>`<th>${button(esc(c.name)+(c.primary?' 🔑':''),`data-studio-sort="${i}"`)}<small>${esc(c.type)}</small></th>`).join('')}</tr></thead><tbody>${page.rows.map((r,i)=>`<tr><td>${page.editable&&page.primary_key.every(name=>typeof r.values[page.columns.findIndex(c=>c.name===name)]!=='object')?button('Edit',`data-studio-edit="${i}"`)+button('Delete',`data-studio-delete="${i}"`):'<span class="studio-muted">Read only</span>'}</td>${r.values.map(v=>`<td>${cell(v)}</td>`).join('')}</tr>`).join('')||`<tr><td colspan="${page.columns.length+1}">No rows on this page. Use Insert to add the first row.</td></tr>`}</tbody></table></div>`;
      $('#studio-prev').onclick=()=>{offset=previousOffsets.pop()??0;loadTable();};$('#studio-next').onclick=()=>{previousOffsets.push(offset);offset+=page.rows.length;loadTable();};$('#studio-csv').onclick=()=>csv(page.columns.map(c=>c.name),page.rows.map(r=>r.values.map(v=>typeof v==='object'&&v!==null?'[binary]':v)));
    }
    function editorFields(row,insert){return page.columns.map((c,i)=>{
      const value=row?.values[i];if(c.generated||typeof value==='object'&&value!==null)return `<p class="studio-muted">${esc(c.name)} · ${c.generated?'generated by the database':'binary value, use a SQL/client binary editor'}</p>`;
      return `<fieldset class="studio-cell-editor"><legend>${esc(c.name)} <small>${esc(c.type)}${c.primary?' · primary key':''}</small></legend><select name="mode_${i}" aria-label="${esc(c.name)} value mode"><option value="keep" ${insert?'selected':''}>${insert?'Use database default':'Keep current value'}</option><option value="value">Set value</option>${c.nullable?'<option value="null">Set NULL</option>':''}</select><textarea name="value_${i}" aria-label="${esc(c.name)} value" rows="2">${esc(value??'')}</textarea></fieldset>`;}).join('');}
    async function saveRow(value,row,insert){
      const values={};page.columns.forEach((c,i)=>{if(value['mode_'+i]==='null')values[c.name]=null;else if(value['mode_'+i]==='value')values[c.name]=value['value_'+i];});
      if(!insert&&!Object.keys(values).length)throw new Error('Choose Set value or Set NULL for a column to change.');
      const key=row?Object.fromEntries(page.primary_key.map(name=>[name,row.values[page.columns.findIndex(c=>c.name===name)]])):{};
      await call({action:insert?'insert':'update',...target(),values,key,token:row?.token});toast(insert?'Row inserted.':'Row saved.');
    }
    function rowForm(row,inline=false){
      if(inline){$('#studio-body').innerHTML=`<form id="studio-insert" class="studio-editor"><p class="studio-muted">Keep “Use database default” for automatic IDs and defaults. Choose “Set NULL” explicitly for an empty database value.</p>${editorFields(null,true)}<button class="studio-button studio-primary">Insert row</button></form>`;$('#studio-insert').onsubmit=async e=>{e.preventDefault();const submit=e.target.querySelector('button');submit.disabled=true;try{await saveRow(Object.fromEntries(new FormData(e.target)),null,true);tab='browse';offset=0;await loadTable();}catch(error){notice(error.message,true);}finally{submit.disabled=false;}};return;}
      modal('Edit row · '+table.name,editorFields(row,false),'Save row',async value=>{await saveRow(value,row,false);close();await loadTable();});
    }
    function structure(){
      $('#studio-body').innerHTML=`<div class="studio-toolbar"><p class="studio-muted">${esc(table.schema)} · ${esc(table.type)} · ${size(table.bytes)} estimated storage</p><div>${table.type==='BASE TABLE'?button('Empty table','data-studio-empty'):''}${button('Add column','id="studio-add-column"')}${button('Drop '+(table.type.endsWith('VIEW')?'view':'table'),'id="studio-drop-table"')}</div></div><div class="studio-scroll"><table class="studio-data"><thead><tr><th>Column</th><th>Type</th><th>Nullable</th><th>Default / identity</th><th>Actions</th></tr></thead><tbody>${page.columns.map((c,i)=>`<tr><td>${esc(c.name)}${c.primary?' 🔑':''}</td><td>${esc(c.type)}</td><td>${c.nullable?'Yes':'No'}</td><td>${esc(c.default??(c.auto?'Automatic':c.generated?'Generated':'—'))}</td><td>${table.type==='BASE TABLE'?button('Rename',`data-studio-rename="${i}"`)+button('Drop',`data-studio-drop-column="${i}"`):'Read only'}</td></tr>`).join('')}</tbody></table></div>`;
      $('#studio-add-column').disabled=table.type!=='BASE TABLE';$('#studio-add-column').onclick=()=>modal('Add column',field('column','Column name')+typeSelect('type')+'<p class="studio-muted">New columns are nullable. Use SQL for advanced defaults and constraints.</p>','Add column',async v=>{await call({action:'alter_table',change:'add',...target(),...v});close();await loadTable();});
      $('#studio-drop-table').onclick=()=>modal('Permanently drop '+table.name,`<p class="mb-4">This removes the ${esc(table.type.toLowerCase())} and its data. Dependent objects are protected by the database; CASCADE is not used.</p>`+field('confirm','Type '+table.name),'Drop permanently',async v=>{await call({action:'drop_table',...target(),confirm:v.confirm});close();tab='browse';offset=0;await loadCatalog();});
    }
    function sqlView(){
      $('#studio-body').innerHTML=`<form id="studio-sql-form" class="studio-editor"><label class="studio-field">SQL workspace<textarea id="studio-sql" rows="10" spellcheck="false">SELECT 1;</textarea></label><p class="studio-muted">Runs as this database’s own role. One statement at a time; up to 500 returned rows / 1 MiB. Truncated statements are rolled back.</p><button class="studio-button studio-primary">Execute SQL</button></form><div id="studio-sql-result" class="studio-scroll" role="status"></div>`;
      $('#studio-sql-form').onsubmit=async e=>{e.preventDefault();const sql=$('#studio-sql').value;const execute=async()=>{const result=await api('/databases/'+db.id+'/query','POST',{sql});if(!live())return;$('#studio-sql-result').innerHTML=`<p class="studio-muted">${result.truncated?'Result truncated; transaction rolled back.':result.affected+' affected / returned rows'}</p><table class="studio-data"><thead><tr>${result.columns.map(c=>`<th>${esc(c)}</th>`).join('')}</tr></thead><tbody>${result.rows.map(r=>`<tr>${r.map(v=>`<td>${cell(v)}</td>`).join('')}</tr>`).join('')}</tbody></table>`;if(!result.columns.length)notice('Statement completed. Refresh the database tree to see schema changes.');};if(!/^\s*(select|show|describe|explain)\b/i.test(sql)){modal('Execute SQL · '+db.name,'<p>This statement can permanently change database data or structure.</p>','Execute',async()=>{await execute();close();});}else{const b=e.target.querySelector('button');b.disabled=true;try{await execute();}catch(error){notice(error.message,true);}finally{b.disabled=false;}}};
    }
    $('#studio-refresh').onclick=()=>loadCatalog(table?target():null);
    $('#studio-create').onclick=()=>{
      let count=0;const column=()=>{const i=count++;return `<div class="studio-new-column"><input name="name_${i}" aria-label="Column name" placeholder="Column name" required value="${i===0?'id':''}">${typeSelect('type_'+i)}<label><input type="checkbox" name="primary_${i}" ${i===0?'checked':''}> Primary key</label><label><input type="checkbox" name="auto_${i}" ${i===0?'checked':''}> Automatic ID</label><label><input type="checkbox" name="null_${i}" ${i?'checked':''}> Nullable</label></div>`;};
      const schemas=[...new Set(['public',...(catalog?.tables||[]).map(t=>t.schema)])];
      modal('Create table · '+db.name,field('table','Table name')+(db.data.engine==='postgresql'?`<label class="studio-field">Schema<select name="schema">${schemas.map(s=>`<option>${esc(s)}</option>`).join('')}</select></label>`:'')+`<div id="studio-new-columns">${column()}</div>`+button('Add another column','id="studio-new-column"'),'Create table',async v=>{const definition=Array.from({length:count},(_,i)=>({name:v['name_'+i],type:v['type_'+i],primary:!!v['primary_'+i],auto:!!v['auto_'+i],nullable:!!v['null_'+i]}));const schema=db.data.engine==='mysql'?catalog.database:v.schema;await call({action:'create_table',schema,table:v.table,columns:definition});close();tab='browse';offset=0;await loadCatalog({schema,table:v.table});});
      $('#studio-new-column').onclick=()=>{if(count<32)$('#studio-new-columns').insertAdjacentHTML('beforeend',column());};
    };
    shell.addEventListener('click',async e=>{
      const el=e.target.closest('button');if(!el||el.disabled)return;
      try{
        if(el.dataset.studioDb){db=databases.find(d=>d.id===el.dataset.studioDb);sessionStorage.setItem('cgp-studio',db.id);table=null;page=null;offset=0;previousOffsets=[];sort='';tab='browse';await loadCatalog();}
        else if(el.dataset.studioTable!==undefined){table=catalog.tables[Number(el.dataset.studioTable)];offset=0;previousOffsets=[];sort='';tab='browse';await loadTable();}
        else if(el.dataset.studioTab){tab=el.dataset.studioTab;if(!table&&tab==='sql'){heading();sqlView();}else paint();}
        else if(el.dataset.studioSort!==undefined){const name=page.columns[Number(el.dataset.studioSort)].name;direction=sort===name&&direction==='asc'?'desc':'asc';sort=name;offset=0;previousOffsets=[];await loadTable();}
        else if(el.dataset.studioEdit!==undefined)rowForm(page.rows[Number(el.dataset.studioEdit)]);
        else if(el.dataset.studioDelete!==undefined){const row=page.rows[Number(el.dataset.studioDelete)],key=Object.fromEntries(page.primary_key.map(name=>[name,row.values[page.columns.findIndex(c=>c.name===name)]]));modal('Delete row · '+table.name,`<p>Permanently delete this row?</p><pre class="my-4 whitespace-pre-wrap">${esc(JSON.stringify(key,null,2))}</pre>`,'Delete row',async()=>{await call({action:'delete',...target(),key,token:row.token});close();await loadTable();});}
        else if(el.hasAttribute('data-studio-empty')){const selected=target(),database=db;modal('Permanently empty '+table.name,'<p class="mb-4">This removes every row from this table. Its columns and structure are retained. Foreign key protection remains active; other tables are never emptied automatically. Back up first.</p>'+field('confirm','Type '+table.name),'Empty table permanently',async v=>{await call({action:'truncate',...selected,confirm:v.confirm},database);close();offset=0;await loadCatalog(selected);});}
        else if(el.dataset.studioRename!==undefined){const column=page.columns[Number(el.dataset.studioRename)].name;modal('Rename column · '+column,field('new_name','New column name',column),'Rename',async v=>{await call({action:'alter_table',change:'rename',...target(),column,new_name:v.new_name});close();await loadTable();});}
        else if(el.dataset.studioDropColumn!==undefined){const column=page.columns[Number(el.dataset.studioDropColumn)].name;modal('Permanently drop column',field('confirm','Type '+column),'Drop column',async v=>{await call({action:'alter_table',change:'drop',...target(),column,confirm:v.confirm});close();await loadTable();});}
      }catch(error){notice(error.message,true);}
    });
    await loadCatalog();
  };
}
