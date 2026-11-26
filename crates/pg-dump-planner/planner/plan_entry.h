#ifndef PGEX_PLAN_ENTRY_H
#define PGEX_PLAN_ENTRY_H
#include <stdint.h>

/* Borrowed from one completed TOC entry until the receiver returns. */
typedef struct {
    int id, section, effective_section, restore_pass;
    uint32_t catalog_table, catalog_oid;
    const char *tag, *description, *definition, *drop_stmt, *copy_stmt;
    const char *namespace_name, *owner, *tablespace, *tableam;
    const int *dependencies;
    int dependency_count;
    char relkind;
} PgexPlanEntry;

typedef struct {
    int id;
    uint32_t table_oid, partition_parent;
    int completes, requires, extension;
} PgexObjectLinks;

typedef void (*PgexRecordObject)(void *, const PgexObjectLinks *, int emitted_id);
typedef void (*PgexReceiveEntry)(void *, const PgexPlanEntry *);
#endif
