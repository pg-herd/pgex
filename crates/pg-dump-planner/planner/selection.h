/* Object identities used by the Rust selector. pg_dump owns these records. */
static const TableInfo *
attachment_table(const DumpableObject *obj)
{
    switch (obj->objType) {
        case DO_TABLE_ATTACH: return ((const TableAttachInfo *) obj)->partitionTbl;
        case DO_ATTRDEF: return ((const AttrDefInfo *) obj)->adtable;
        case DO_INDEX: return ((const IndxInfo *) obj)->indextable;
        case DO_INDEX_ATTACH: return ((const IndexAttachInfo *) obj)->partitionIdx->indextable;
        case DO_STATSEXT: return ((const StatsExtInfo *) obj)->stattable;
        case DO_RULE: return ((const RuleInfo *) obj)->ruletable;
        case DO_TRIGGER: return ((const TriggerInfo *) obj)->tgtable;
        case DO_CONSTRAINT:
        case DO_FK_CONSTRAINT: return ((const ConstraintInfo *) obj)->contable;
        case DO_POLICY: return ((const PolicyInfo *) obj)->poltable;
        case DO_PUBLICATION_REL: return ((const PublicationRelInfo *) obj)->pubtable;
        case DO_TABLE_DATA:
        case DO_SEQUENCE_SET:
        case DO_REFRESH_MATVIEW: return ((const TableDataInfo *) obj)->tdtable;
        default: return NULL;
    }
}

static void
record_object(PgexRecordObject record, void *context, DumpableObject *obj,
              TocEntry *before, TocEntry *last)
{
    PgexObjectLinks links = { .id = obj->dumpId };
    const TableInfo *table = attachment_table(obj);
    if (table)
        links.table_oid = table->dobj.catId.oid;

    /* Some definitions are emitted in pieces. Keep the completion even when
     * the original object was reached only as another object's prerequisite. */
    switch (obj->objType) {
        case DO_TABLE: {
            TableInfo *relation = (TableInfo *) obj;
            links.table_oid = obj->catId.oid;
            if (relation->ispartition && relation->numParents == 1)
                links.partition_parent = relation->parents[0]->dobj.catId.oid;
            if (OidIsValid(relation->owning_tab)) {
                TableInfo *owner = findTableByOid(relation->owning_tab);
                links.table_oid = relation->owning_tab;
                if (owner) {
                    links.requires = owner->dobj.dumpId;
                    links.completes = owner->dobj.dumpId;
                }
            }
            break;
        }
        case DO_ATTRDEF:
        case DO_TABLE_ATTACH:
            links.completes = table->dobj.dumpId;
            break;
        case DO_RULE:
            if (((RuleInfo *)obj)->ev_type == '1' && table->dummy_view)
                links.completes = table->dobj.dumpId;
            break;
        case DO_INDEX_ATTACH:
            links.completes = ((IndexAttachInfo *)obj)->partitionIdx->dobj.dumpId;
            break;
        case DO_CONSTRAINT: {
            ConstraintInfo *con = (ConstraintInfo *) obj;
            if (con->condomain)
                links.completes = con->condomain->dobj.dumpId;
            else if (table && con->contype == 'c')
                links.completes = table->dobj.dumpId;
            break;
        }
        default: break;
    }
    if (obj->ext_member) {
        ExtensionInfo *extension = findOwningExtension(obj->catId);
        if (extension)
            links.extension = extension->dobj.dumpId;
    }
    record(context, &links, 0);
    /* Includes synthetic entries such as column ACLs and SEQUENCE OWNED BY. */
    while (before != last) {
        before = before->next;
        record(context, &links, before->dumpId);
    }
}
