#include "pg_backup_archiver.c"

int pgex_restore_pass(TocEntry *entry)
{
    return _tocEntryRestorePass(entry);
}
