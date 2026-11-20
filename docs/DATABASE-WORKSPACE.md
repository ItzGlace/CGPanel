# Database Studio and account removal

Open **Databases** to create a MySQL or PostgreSQL database and see storage totals. Select **Database Studio**, choose a database in the left sidebar, then choose a table. Use the table filter to search larger schemas.

- **Browse** shows 50 rows per page. Click a column heading to sort, use Previous/Next to navigate, or export the current page as CSV. Edit/Delete identify rows by the complete primary key. A view or table without a primary key is read only in Browse; SQL remains available.
- **Structure** lists columns, types, nullability, defaults and primary keys. Add a nullable column, rename one or drop one with a typed confirmation. Use SQL for indexes, constraints, advanced defaults, triggers and views.
- **Insert** offers a separate choice for database defaults, a supplied value and SQL NULL. Automatic ID columns can keep the default. Generated and binary values require an appropriate SQL/client workflow.
- **New table** builds a table using supported column types, primary keys and optional automatic integer IDs. PostgreSQL supports schema selection.
- **Empty table** removes all rows and keeps structure. **Drop table** removes structure and data. Both require the exact table name. Dependent tables are never automatically cascaded. Back up first.
- **SQL** executes one statement as the database's own role, with a 15-second engine timeout and at most 500 rows / 1 MiB. Truncated SQL results are rolled back. Binary editing, large imports and advanced engine features should use a database client through the configured access policy.

Updates reject a row that changed after you opened it. Refresh and review the new value before editing again. Database requests cannot access another panel database or obtain host administrator privileges.

Storage uses decimal units: 1 KB = 1,000 bytes, 1 MB = 1,000,000 bytes, then GB/TB/PB/EB. Hover over a size for exact bytes. MySQL values are engine table/index estimates and can lag changes; PostgreSQL database totals include storage overhead. Views generally have no independent storage. Table row counts are estimates; navigation uses actual query results.

## Permanent account deletion

As an administrator, open **Users → Delete permanently**. Review the resource counts, domains and blockers. Enter the exact username and check the permanent-removal acknowledgement. Administrator accounts and your own account cannot be deleted here.

The account is suspended and sessions/API/MCP/OAuth credentials are revoked immediately. A background job stops only this account's services, detaches its storage, removes its databases/mail/files/container store and local backups, then removes panel metadata. Follow its result under **Jobs**. Running jobs must finish before deletion. Unexpected mounts, shared OS identities and dependencies from another account block deletion for review.

Failure leaves the account suspended and records the error. Resolve it, open a fresh deletion preview in Users and confirm again. Cleanup resumes from a root-owned plan. Audit records, server-wide update snapshots and copies at external backup destinations are retained; deleting an account does not securely erase SSD media or remote backups.

Suspension alone keeps workloads and data. Use it when access should be revoked without erasing a project.
