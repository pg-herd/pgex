#ifndef PGEX_TABLE_LAYOUT_H
#define PGEX_TABLE_LAYOUT_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct {
    const char *name, *formatted_type, *expression;
    int number;
    uint32_t collation_oid;
    char identity, generated;
    bool not_null;
} PgexColumnLayout;

/* The receiver copies this view before it returns. */
typedef struct {
    const char *schema, *name;
    const PgexColumnLayout *columns;
    size_t column_count;
    uint32_t relation_oid;
    char relation_kind;
} PgexTableLayout;

typedef void (*PgexReceiveTableLayout)(void *, const PgexTableLayout *);
#endif
