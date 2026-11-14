WITH requested AS (
    SELECT DISTINCT schema_name, table_name
    FROM ROWS FROM (pg_catalog.unnest($1::text[]), pg_catalog.unnest($2::text[]))
        AS names(schema_name, table_name)
), columns AS (
    SELECT n.nspname::text AS schema_name, c.relname::text AS table_name,
           c.oid AS relation_oid, a.attnum, a.attname::text AS column_name,
           a.atttypid, a.atttypmod AS type_modifier,
           NOT a.attnotnull AS nullable, a.attgenerated <> '' AS generated
    FROM requested r
    JOIN pg_catalog.pg_namespace n ON n.nspname = r.schema_name
    JOIN pg_catalog.pg_class c ON c.relnamespace = n.oid AND c.relname = r.table_name
    LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
        AND a.attnum > 0 AND NOT a.attisdropped
    WHERE c.relkind IN ('r', 'p')
), enum_definitions AS (
    SELECT e.enumtypid, pg_catalog.array_agg(e.enumlabel::text ORDER BY e.enumsortorder) AS labels
    FROM pg_catalog.pg_enum e
    JOIN (SELECT DISTINCT atttypid FROM columns) types ON types.atttypid = e.enumtypid
    GROUP BY e.enumtypid
)
SELECT c.schema_name, c.table_name, c.relation_oid, c.column_name,
       n.nspname::text AS type_schema, t.typname::text AS type_name,
       c.type_modifier, c.nullable, c.generated,
       CASE WHEN t.typtype = 'e' THEN coalesce(e.labels, ARRAY[]::text[]) END AS enum_labels
FROM columns c
LEFT JOIN pg_catalog.pg_type t ON t.oid = c.atttypid
LEFT JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace
LEFT JOIN enum_definitions e ON e.enumtypid = t.oid
ORDER BY c.schema_name COLLATE pg_catalog."C", c.table_name COLLATE pg_catalog."C", c.attnum
