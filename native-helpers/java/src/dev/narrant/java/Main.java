package dev.narrant.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import com.google.gson.Strictness;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonToken;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.StringReader;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;

/** Java 静态分析进程；一次启动只处理一个请求。 */
public final class Main {
    static final String PROTOCOL = "narrant.native-analyzer/1";
    static final String PROFILE = "java-static-v1";
    private static final int MAX_INPUT_CHARS = 1_048_576;

    /** 防止工具类被实例化。 */
    private Main() {}

    /** 启动握手、分析和有界取消监听。 */
    public static void main(String[] args) throws Exception {
        if (args.length != 0) throw new IllegalArgumentException("unexpected arguments");
        BufferedReader input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        JsonObject ready = base("ready");
        ready.addProperty("helper_id", "narrant-java");
        ready.addProperty("helper_version", "0.1.0");
        ready.addProperty("projection_revision", "java-source-v1");
        ready.add("languages", strings(List.of("java")));
        ready.add("profiles", strings(List.of(PROFILE)));
        emit(ready);
        String line = input.readLine();
        if (line == null) return;
        JsonObject request;
        String id;
        try {
            request = parseObject(line);
            fields(request, Set.of("protocol", "type", "id", "input", "environment", "limits"));
            requireString(request, "protocol", PROTOCOL);
            requireString(request, "type", "analyze");
            id = string(request, "id");
            if (id.isEmpty()) throw new Failure("invalid_input", "empty request id");
        } catch (Failure problem) {
            throw new IllegalArgumentException(problem.getMessage(), problem);
        }
        AtomicBoolean cancelled = new AtomicBoolean();
        AtomicBoolean invalidCancel = new AtomicBoolean();
        Thread listener = new Thread(() -> listen(input, id, cancelled, invalidCancel), "narrant-java-cancel");
        listener.setDaemon(true);
        listener.start();
        JsonObject completed = base("completed");
        completed.addProperty("id", id);
        try {
            Analyzer analyzer = new Analyzer(request, cancelled);
            JsonObject result = analyzer.run();
            if (invalidCancel.get()) throw new Failure("invalid_input", "invalid cancel frame");
            if (cancelled.get()) {
                completed.addProperty("outcome", "cancelled");
            } else {
                completed.addProperty("outcome", "ok");
                completed.add("graph", result.get("graph"));
                completed.add("evidence", result.get("evidence"));
            }
            if (completed.toString().getBytes(StandardCharsets.UTF_8).length > analyzer.maxResultBytes())
                throw new Failure("resource_limit", "result exceeds max_result_bytes");
        } catch (Failure problem) {
            if (cancelled.get() && !invalidCancel.get()) {
                completed.addProperty("outcome", "cancelled");
            } else {
                completed.addProperty("outcome", "error");
                JsonObject error = new JsonObject();
                error.addProperty("code", problem.code);
                error.addProperty("message", problem.getMessage());
                error.add("paths", strings(problem.paths));
                completed.add("error", error);
            }
        } catch (Throwable problem) {
            completed.addProperty("outcome", "error");
            JsonObject error = new JsonObject();
            error.addProperty("code", "internal_failure");
            error.addProperty("message", problem.getClass().getSimpleName() + ": " + problem.getMessage());
            error.add("paths", new JsonArray());
            completed.add("error", error);
        }
        emit(completed);
    }

    /** 在分析期间只接受同一请求的取消帧。 */
    private static void listen(BufferedReader input, String id, AtomicBoolean cancelled,
                               AtomicBoolean invalidCancel) {
        try {
            String line = input.readLine();
            if (line == null) return;
            JsonObject frame = parseObject(line);
            fields(frame, Set.of("protocol", "type", "id"));
            requireString(frame, "protocol", PROTOCOL);
            requireString(frame, "type", "cancel");
            requireString(frame, "id", id);
            cancelled.set(true);
            if (input.ready() && input.readLine() != null) invalidCancel.set(true);
        } catch (Exception problem) {
            invalidCancel.set(true);
            cancelled.set(true);
        }
    }

    /** 创建带固定协议标识的帧。 */
    static JsonObject base(String type) {
        JsonObject object = new JsonObject();
        object.addProperty("protocol", PROTOCOL);
        object.addProperty("type", type);
        return object;
    }

    /** 向协议输出发送唯一完整 JSON 行。 */
    private static void emit(JsonObject message) {
        System.out.println(message);
        System.out.flush();
    }

    /** 读取有限 JSON 对象并拒绝重复键。 */
    static JsonObject parseObject(String line) throws Failure {
        if (line.length() > MAX_INPUT_CHARS) throw new Failure("invalid_input", "input frame too large");
        try {
            JsonReader reader = new JsonReader(new StringReader(line));
            reader.setStrictness(Strictness.STRICT);
            JsonElement parsed = readJson(reader, 0);
            if (reader.peek() != JsonToken.END_DOCUMENT) throw new IllegalArgumentException("trailing JSON");
            if (!parsed.isJsonObject()) throw new IllegalArgumentException("expected JSON object");
            return parsed.getAsJsonObject();
        } catch (Exception problem) {
            throw new Failure("invalid_input", "invalid JSON frame");
        }
    }

    /** 递归读取 JSON 并在每层拒绝重复键与过深输入。 */
    private static JsonElement readJson(JsonReader reader, int depth) throws Exception {
        if (depth > 64) throw new IllegalArgumentException("JSON nesting too deep");
        return switch (reader.peek()) {
            case BEGIN_OBJECT -> {
                JsonObject object = new JsonObject();
                reader.beginObject();
                while (reader.hasNext()) {
                    String key = reader.nextName();
                    if (object.has(key)) throw new IllegalArgumentException("duplicate JSON key");
                    object.add(key, readJson(reader, depth + 1));
                }
                reader.endObject();
                yield object;
            }
            case BEGIN_ARRAY -> {
                JsonArray array = new JsonArray();
                reader.beginArray();
                while (reader.hasNext()) array.add(readJson(reader, depth + 1));
                reader.endArray();
                yield array;
            }
            case STRING -> new JsonPrimitive(reader.nextString());
            case NUMBER -> new JsonPrimitive(new BigDecimal(reader.nextString()));
            case BOOLEAN -> new JsonPrimitive(reader.nextBoolean());
            case NULL -> { reader.nextNull(); yield JsonNull.INSTANCE; }
            default -> throw new IllegalArgumentException("invalid JSON token");
        };
    }

    /** 验证对象字段闭集。 */
    static void fields(JsonObject object, Set<String> allowed) throws Failure {
        for (String key : object.keySet()) if (!allowed.contains(key))
            throw new Failure("invalid_input", "unknown field: " + key);
        for (String key : allowed) if (!object.has(key))
            throw new Failure("invalid_input", "missing field: " + key);
    }

    /** 提取必填字符串。 */
    static String string(JsonObject object, String key) throws Failure {
        if (!object.has(key) || !object.get(key).isJsonPrimitive() || !object.get(key).getAsJsonPrimitive().isString())
            throw new Failure("invalid_input", "expected string: " + key);
        return object.get(key).getAsString();
    }

    /** 验证固定值字符串。 */
    static void requireString(JsonObject object, String key, String expected) throws Failure {
        if (!expected.equals(string(object, key)))
            throw new Failure("invalid_input", "unsupported " + key);
    }

    /** 提取必填对象。 */
    static JsonObject object(JsonObject parent, String key) throws Failure {
        if (!parent.has(key) || !parent.get(key).isJsonObject())
            throw new Failure("invalid_input", "expected object: " + key);
        return parent.getAsJsonObject(key);
    }

    /** 提取必填数组。 */
    static JsonArray array(JsonObject parent, String key) throws Failure {
        if (!parent.has(key) || !parent.get(key).isJsonArray())
            throw new Failure("invalid_input", "expected array: " + key);
        return parent.getAsJsonArray(key);
    }

    /** 将字符串集合编码为协议数组。 */
    static JsonArray strings(Iterable<String> values) {
        JsonArray result = new JsonArray();
        for (String value : values) result.add(value);
        return result;
    }

    /** 受控失败及其仓内相对来源。 */
    static final class Failure extends Exception {
        final String code;
        final List<String> paths;

        /** 建立无路径错误。 */
        Failure(String code, String message) { this(code, message, List.of()); }

        /** 建立有路径错误。 */
        Failure(String code, String message, List<String> paths) {
            super(message);
            this.code = code;
            this.paths = paths;
        }
    }
}
