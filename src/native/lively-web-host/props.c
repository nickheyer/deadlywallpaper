#include "props.h"

#include <string.h>

#define LWH_PROPS_ERROR lwh_props_error_quark()

static GQuark lwh_props_error_quark(void)
{
    return g_quark_from_static_string("lively-web-host-props");
}

static JsonNode *member(JsonObject *control, const gchar *key)
{
    JsonNode *node = json_object_get_member(control, key);

    if (node == NULL || JSON_NODE_HOLDS_NULL(node))
        return NULL;
    return node;
}

static gboolean number_of(JsonNode *node, gdouble *out)
{
    GType t;

    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node))
        return FALSE;
    t = json_node_get_value_type(node);
    if (t == G_TYPE_INT64 || t == G_TYPE_DOUBLE) {
        *out = json_node_get_double(node);
        return TRUE;
    }
    if (t == G_TYPE_STRING) {
        const gchar *s = json_node_get_string(node);
        gchar *end = NULL;
        gdouble v = g_ascii_strtod(s, &end);

        if (s != NULL && *s != '\0' && end != NULL && *end == '\0') {
            *out = v;
            return TRUE;
        }
    }
    return FALSE;
}

static gboolean boolean_of(JsonNode *node, gboolean *out)
{
    GType t;

    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node))
        return FALSE;
    t = json_node_get_value_type(node);
    if (t == G_TYPE_BOOLEAN) {
        *out = json_node_get_boolean(node);
        return TRUE;
    }
    if (t == G_TYPE_STRING) {
        const gchar *s = json_node_get_string(node);

        if (g_ascii_strcasecmp(s, "true") == 0) {
            *out = TRUE;
            return TRUE;
        }
        if (g_ascii_strcasecmp(s, "false") == 0) {
            *out = FALSE;
            return TRUE;
        }
    }
    return FALSE;
}

/* Newtonsoft turns numbers into strings for string properties; anything else is an error. */
static gboolean string_of(JsonNode *node, gchar **out)
{
    GType t;

    if (node == NULL) {
        *out = NULL;
        return TRUE;
    }
    if (!JSON_NODE_HOLDS_VALUE(node))
        return FALSE;
    t = json_node_get_value_type(node);
    if (t == G_TYPE_STRING) {
        *out = g_strdup(json_node_get_string(node));
        return TRUE;
    }
    if (t == G_TYPE_INT64 || t == G_TYPE_DOUBLE) {
        *out = json_to_string(node, FALSE);
        return TRUE;
    }
    return FALSE;
}

static JsonNode *string_node(const gchar *value)
{
    JsonNode *node = json_node_new(value != NULL ? JSON_NODE_VALUE : JSON_NODE_NULL);

    if (value != NULL)
        json_node_set_string(node, value);
    return node;
}

/* LivelyPropertyUtil.GetFolderDropdownValue */
static JsonNode *folder_dropdown_value(JsonObject *control, const gchar *root_dir)
{
    JsonNode *value_node = member(control, "value");
    JsonNode *folder_node = member(control, "folder");
    const gchar *value = value_node != NULL && JSON_NODE_HOLDS_VALUE(value_node) &&
                         json_node_get_value_type(value_node) == G_TYPE_STRING ? json_node_get_string(value_node) : NULL;
    const gchar *folder = folder_node != NULL && JSON_NODE_HOLDS_VALUE(folder_node) &&
                          json_node_get_value_type(folder_node) == G_TYPE_STRING ? json_node_get_string(folder_node) : NULL;
    gchar *relative;
    gchar *full;
    JsonNode *result;

    if (value == NULL || folder == NULL || root_dir == NULL)
        return string_node(NULL);

    relative = g_build_filename(folder, value, NULL);
    full = g_build_filename(root_dir, relative, NULL);
    result = string_node(g_file_test(full, G_FILE_TEST_IS_REGULAR) ? relative : NULL);
    g_free(full);
    g_free(relative);
    return result;
}

static gboolean control_value(const gchar *name, JsonObject *control, const gchar *root_dir,
                              JsonNode **value, gboolean *skip, GError **error)
{
    JsonNode *type_node = member(control, "type");
    gchar *type;
    gboolean ok = TRUE;

    *skip = FALSE;
    *value = NULL;
    if (type_node == NULL || !JSON_NODE_HOLDS_VALUE(type_node) || json_node_get_value_type(type_node) != G_TYPE_STRING) {
        g_set_error(error, LWH_PROPS_ERROR, 0, "Control '%s' has no type", name);
        return FALSE;
    }
    type = g_ascii_strdown(json_node_get_string(type_node), -1);

    if (strcmp(type, "slider") == 0) {
        gdouble v = 0.0;
        JsonNode *raw = member(control, "value");

        if (raw != NULL && !number_of(raw, &v))
            ok = FALSE;
        else {
            *value = json_node_new(JSON_NODE_VALUE);
            json_node_set_double(*value, v);
        }
    } else if (strcmp(type, "dropdown") == 0) {
        gdouble v = 0.0;
        JsonNode *raw = member(control, "value");

        if (raw != NULL && (!number_of(raw, &v) || v != (gdouble)(gint64)v))
            ok = FALSE;
        else {
            *value = json_node_new(JSON_NODE_VALUE);
            json_node_set_int(*value, (gint64)v);
        }
    } else if (strcmp(type, "folderdropdown") == 0) {
        *value = folder_dropdown_value(control, root_dir);
    } else if (strcmp(type, "checkbox") == 0) {
        gboolean v = FALSE;
        JsonNode *raw = member(control, "value");

        if (raw != NULL && !boolean_of(raw, &v))
            ok = FALSE;
        else {
            *value = json_node_new(JSON_NODE_VALUE);
            json_node_set_boolean(*value, v);
        }
    } else if (strcmp(type, "textbox") == 0 || strcmp(type, "color") == 0) {
        gchar *s = NULL;

        if (!string_of(member(control, "value"), &s))
            ok = FALSE;
        else {
            *value = string_node(s);
            g_free(s);
        }
    } else if (strcmp(type, "button") == 0 || strcmp(type, "label") == 0) {
        *skip = TRUE;
    } else {
        g_set_error(error, LWH_PROPS_ERROR, 0, "Unsupported control type: %s", json_node_get_string(type_node));
        ok = FALSE;
    }

    if (!ok && error != NULL && *error == NULL)
        g_set_error(error, LWH_PROPS_ERROR, 0, "Control '%s' (%s) has a value of the wrong type", name, type);
    g_free(type);
    return ok;
}

gboolean lwh_props_load(const gchar *property_path, const gchar *root_dir,
                        LwhPropApplyFunc apply, gpointer user_data, GError **error)
{
    JsonParser *parser;
    JsonNode *root;
    JsonObjectIter iter;
    const gchar *name;
    JsonNode *control_node;
    gboolean ok = TRUE;

    if (property_path == NULL || !g_file_test(property_path, G_FILE_TEST_IS_REGULAR))
        return TRUE;

    parser = json_parser_new();
    if (!json_parser_load_from_file(parser, property_path, error)) {
        g_object_unref(parser);
        return FALSE;
    }
    root = json_parser_get_root(parser);
    if (root == NULL || !JSON_NODE_HOLDS_OBJECT(root)) {
        g_set_error(error, LWH_PROPS_ERROR, 0, "%s: top level is not a JSON object", property_path);
        g_object_unref(parser);
        return FALSE;
    }

    json_object_iter_init_ordered(&iter, json_node_get_object(root));
    while (ok && json_object_iter_next_ordered(&iter, &name, &control_node)) {
        JsonNode *value = NULL;
        gboolean skip = FALSE;

        if (!JSON_NODE_HOLDS_OBJECT(control_node)) {
            g_set_error(error, LWH_PROPS_ERROR, 0, "Control '%s' is not a JSON object", name);
            ok = FALSE;
            break;
        }
        ok = control_value(name, json_node_get_object(control_node), root_dir, &value, &skip, error);
        if (ok && !skip)
            apply(name, value, user_data);
        if (value != NULL)
            json_node_unref(value);
    }

    g_object_unref(parser);
    return ok;
}
