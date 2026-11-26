/* One invocation in the private pgex planner child on a Rust-owned connection. */
#include "postgres_fe.h"
#include <time.h>
#include "common/logging.h"
#include "pg_backup_archiver.h"
#include "fe_utils/string_utils.h"

typedef struct {
    bool standby, full, select_dependencies;
    const Oid *selected;
    size_t selected_count;
    const char *snapshot;
} PgexSettings;

#define main pgex_unused_dump_main
#include "pg_dump.c"
#undef main
#include "plan_entry.h"
#include "selection.h"
#include "table_layout.h"

extern int pgex_restore_pass(TocEntry *te);

static void emit_entry(PgexReceiveEntry receive, void *context, const TocEntry *te, int section)
{
    PgexPlanEntry entry = {
        .id = te->dumpId, .section = te->section,
        .effective_section = section, .restore_pass = pgex_restore_pass((TocEntry *) te),
        .catalog_table = te->catalogId.tableoid, .catalog_oid = te->catalogId.oid,
        .tag = te->tag, .description = te->desc, .definition = te->defn,
        .drop_stmt = te->dropStmt, .copy_stmt = te->copyStmt,
        .namespace_name = te->namespace, .owner = te->owner, .tablespace = te->tablespace,
        .tableam = te->tableam, .dependencies = te->dependencies,
        .dependency_count = te->nDeps, .relkind = te->relkind
    };
    receive(context, &entry);
}

static void emit_table_layout(PgexReceiveTableLayout receive, void *context, const TableInfo *table)
{
    PgexColumnLayout *columns = NULL;
    size_t count = 0;

    if (table->numatts > 0)
        columns = pg_malloc_array(PgexColumnLayout, table->numatts);
    for (int i = 0; i < table->numatts; i++) {
        if (table->attisdropped[i])
            continue;
        columns[count++] = (PgexColumnLayout) {
            .name = table->attnames[i],
            .formatted_type = table->atttypnames[i],
            .expression = table->attrdefs[i] ? table->attrdefs[i]->adef_expr : NULL,
            .number = i + 1,
            .collation_oid = table->attcollation[i],
            .identity = table->attidentity[i],
            .generated = table->attgenerated[i],
            .not_null = table->notnull_constrs[i] != NULL || table->notnull_invalid[i],
        };
    }
    PgexTableLayout layout = {
        .schema = table->dobj.namespace->dobj.name,
        .name = table->dobj.name,
        .columns = columns,
        .column_count = count,
        .relation_oid = table->dobj.catId.oid,
        .relation_kind = table->relkind,
    };
    receive(context, &layout);
    pg_free(columns);
}

/* CLOCK_MONOTONIC is shared by the parent and child on this host. */
static uint64_t monotonic_ns(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        pg_fatal("could not read monotonic clock: %m");
    return (uint64_t) now.tv_sec * UINT64_C(1000000000) + now.tv_nsec;
}

void pgex_child_plan(const PgexSettings *settings, PGconn *connection,
                     void *context, PgexReceiveEntry receive, PgexRecordObject record,
                     PgexReceiveTableLayout receive_table, uint64_t *discovery_start,
                     uint64_t *sort_start, uint64_t *actions_start)
{
    bool full = settings->full;
    progname = "pgex";
    pg_logging_init(progname);
    pg_logging_set_level(PG_LOG_WARNING);
    ArchiveHandle archive = {0};
    archive.connection = connection;
    Archive *fout = &archive.public;
    DumpOptions dopt;
    InitDumpOptions(&dopt);
    dopt.dumpData = full;
    bool filter_clone = settings->select_dependencies && settings->selected_count > 0;
    dopt.include_everything = settings->select_dependencies || settings->selected_count == 0;
    dopt.outputLOs = full && dopt.include_everything;
    for (size_t i = 0; !settings->select_dependencies && i < settings->selected_count; i++)
        simple_oid_list_append(&table_include_oids, settings->selected[i]);
    dopt.outputCreateDB = true;
    archive.mode = archModeWrite;
    archive.format = archCustom;
    archive.version = K_VERS_SELF;
    TocEntry head = {0};
    archive.toc = head.prev = head.next = &head;
    fout->numWorkers = 1;
    fout->minRemoteVersion = 90200;
    fout->maxRemoteVersion = 180099;
    SetArchiveOptions(fout, &dopt, NULL);
    fout->remoteVersion = PQserverVersion(connection);
    fout->remoteVersionStr = pg_strdup(PQparameterStatus(connection, "server_version"));
    archive.archiveRemoteVersion = fout->remoteVersionStr;
    fout->isStandby = settings->standby;
    setup_connection(fout, NULL, settings->snapshot, NULL);
    if (fout->isStandby)
        dopt.no_unlogged_table_data = true;
    g_last_builtin_oid = FirstNormalObjectId - 1;
    *discovery_start = monotonic_ns();
    collectRoleNames(fout);
    int numTables, numObjs;
    TableInfo *tables = getSchemaData(fout, &numTables);
    for (int i = 0; i < numTables; i++) {
        const TableInfo *table = &tables[i];
        if (table->interesting &&
            (table->relkind == RELKIND_RELATION || table->relkind == RELKIND_PARTITIONED_TABLE))
            emit_table_layout(receive_table, context, table);
    }
    if (full) {
        getTableData(&dopt, tables, numTables, 0);
        buildMatViewRefreshDependencies(fout);
        if (dopt.outputLOs)
            getLOs(fout);
    }
    getDependencies(fout);
    getAdditionalACLs(fout);
    collectComments(fout);
    collectSecLabels(fout);
    collectSequences(fout);
    *sort_start = monotonic_ns();
    DumpableObject *boundaries = createBoundaryObjects();
    DumpableObject **objects;
    getDumpableObjects(&objects, &numObjs);
    addBoundaryDependencies(objects, numObjs, boundaries);
    sortDumpableObjectsByTypeName(objects, numObjs);
    sortDumpableObjects(objects, numObjs, boundaries[0].dumpId, boundaries[1].dumpId);
    *actions_start = monotonic_ns();
    dumpEncoding(fout);
    dumpStdStrings(fout);
    dumpSearchPath(fout);
    dumpDatabase(fout);
    if (filter_clone) {
        /* Entries emitted before the object loop are database/session setup. */
        PgexObjectLinks bootstrap = {0};
        for (TocEntry *te = head.next; te != &head; te = te->next)
            record(context, &bootstrap, te->dumpId);
    }
    for (int i = 0; i < numObjs; i++) {
        TocEntry *previous = head.prev;
        dumpDumpableObject(fout, objects[i]);
        if (filter_clone)
            record_object(record, context, objects[i], previous, head.prev);
    }
    RestoreOptions *ropt = NewRestoreOptions();
    ropt->dumpData = full;
    ropt->dumpStatistics = false;
    ropt->createDB = true;
    ropt->include_everything = dopt.include_everything;
    ropt->suppressDumpWarnings = true;
    SetArchiveOptions(fout, &dopt, ropt);
    ProcessArchiveRestoreOptions(fout);
    BuildArchiveDependencies(fout);
    teSection section = SECTION_PRE_DATA;
    for (TocEntry *te = head.next; te != &head; te = te->next) {
        if (!te->reqs) continue;
        if (te->section != SECTION_NONE) section = te->section;
        if (filter_clone || section != SECTION_DATA)
            emit_entry(receive, context, te, section);
    }
}
