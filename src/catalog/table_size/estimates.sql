WITH requested AS (
    SELECT DISTINCT schema_name, table_name
    FROM ROWS FROM (pg_catalog.unnest($1::text[]), pg_catalog.unnest($2::text[]))
        AS names(schema_name, table_name)
)
SELECT n.nspname::text AS schema_name, c.relname::text AS table_name,
       c.reltuples::float8 AS total_rows, widths.bytes_per_row,
       -- Column widths include the inline pointer for out-of-line values.
       CASE WHEN c.reltoastrelid <> 0 THEN pg_catalog.pg_relation_size(c.reltoastrelid)
            ELSE 0 END AS toast_bytes,
       tenant_stats.null_frac AS tenant_null_fraction,
       tenant_stats.n_distinct AS tenant_distinct,
       CASE WHEN tenant_stats.attname IS NOT NULL THEN
           coalesce(tenant_stats.most_common_vals::text::bigint[], ARRAY[]::bigint[])
       END AS common_ids,
       coalesce(tenant_stats.most_common_freqs, ARRAY[]::real[]) AS common_frequencies
FROM requested r
JOIN pg_catalog.pg_namespace n ON n.nspname = r.schema_name
JOIN pg_catalog.pg_class c ON c.relnamespace = n.oid AND c.relname = r.table_name
LEFT JOIN LATERAL (
    SELECT CASE WHEN count(*) = count(s.attname) THEN
        2.0 + 4.0 * count(*) + coalesce(sum(s.avg_width * (1.0 - s.null_frac::float8)), 0.0)
    END AS bytes_per_row
    FROM pg_catalog.pg_attribute a
    LEFT JOIN pg_catalog.pg_stats s ON s.schemaname = n.nspname AND s.tablename = c.relname
        AND s.attname = a.attname AND NOT s.inherited
    WHERE a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped AND a.attgenerated = ''
) widths ON true
LEFT JOIN pg_catalog.pg_attribute tenant ON tenant.attrelid = c.oid AND tenant.attname = $3::text
    AND tenant.attnum > 0 AND NOT tenant.attisdropped
    AND tenant.atttypid IN ('pg_catalog.int2'::regtype, 'pg_catalog.int4'::regtype, 'pg_catalog.int8'::regtype)
LEFT JOIN pg_catalog.pg_stats tenant_stats ON tenant_stats.schemaname = n.nspname
    AND tenant_stats.tablename = c.relname AND tenant_stats.attname = tenant.attname
    AND NOT tenant_stats.inherited
WHERE c.relkind = 'r'
    AND NOT EXISTS (SELECT FROM pg_catalog.pg_inherits i WHERE i.inhparent = c.oid)
