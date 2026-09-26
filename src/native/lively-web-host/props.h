#ifndef LWH_PROPS_H
#define LWH_PROPS_H

#include <glib.h>
#include <json-glib/json-glib.h>

/* Called once per applicable control with the value LivelyPropertyUtil.LoadProperty
 * would pass to livelyPropertyListener. value is owned by the loader. */
typedef void (*LwhPropApplyFunc)(const gchar *name, JsonNode *value, gpointer user_data);

/* C port of LivelyPropertyUtil.LoadProperty(string, string, ExecuteScriptDelegate).
 * A missing file is a no-op that returns TRUE. Any parse problem or unsupported
 * control aborts the walk and returns FALSE with *error set. */
gboolean lwh_props_load(const gchar *property_path, const gchar *root_dir,
                        LwhPropApplyFunc apply, gpointer user_data, GError **error);

#endif
