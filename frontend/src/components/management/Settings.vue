<script setup>
import { ref, reactive, onMounted, onUnmounted, provide } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ElMessage } from "element-plus";
import { copyProperties, httpReq, getRobotType } from "../../assets/tools.js";
import { useI18n } from "vue-i18n";
import chatPicThumbnail from "@/assets/usedByLlmChatNode-thumbnail.png";
import chatPic from "@/assets/usedByLlmChatNode.png";
import sentenceEmbeddingPicThumbnail from "@/assets/usedBySentenceEmbedding-thumbnail.png";
import sentenceEmbeddingPic from "@/assets/usedBySentenceEmbedding.png";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const robotId = route.params.robotId;
const robotType = getRobotType(robotId);
const maxSessionIdleMin = ref(30);

const goBack = () => {
    router.push({ name: "robotDetail", params: { robotId: robotId } });
};

const similarityThreshold = ref(85);
const defaultEmailVerificationRegex =
    "[-\\w\\.\\+]{1,100}@[A-Za-z0-9]{1,30}[A-Za-z\\.]{2,30}";
const settings = reactive({
    version: 1017000,
    maxSessionIdleSec: 1800,
    smtpHost: "",
    smtpUsername: "",
    smtpPassword: "",
    smtpTimeoutSec: 60,
    emailVerificationRegex: "",
    chatProvider: {
        provider: {
            id: "",
            model: "",
        },
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        max_token_len: 1000,
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        maxResponseTokenLength: 5000,
        proxyUrl: "",
    },
    sentenceEmbeddingProvider: {
        provider: {
            id: "",
            model: "",
        },
        similarityThreshold: 0.85,
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        proxyUrl: "",
    },
    asrProvider: {
        enabled: false,
        provider: {
            id: "",
            model: "",
        },
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        proxyUrl: "",
    },
    ttsProvider: {
        enabled: false,
        provider: {
            id: "",
            model: "",
        },
        apiUrl: "",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        apiKey: "",
        connectTimeoutMillis: 5000,
        readTimeoutMillis: 10000,
        proxyUrl: "",
    },
});
const formLabelWidth = "150px";
const loading = ref(false);
const smtpPassed = ref(false);
const smtpFailed = ref(false);
const smtpFailedDetail = ref("");
const showHfIncorrectChatModelTip = ref(false);
const showHfChatModelDownloadProgress = ref(false);
const chatModelRepository = ref("");
const showHfIncorrectEmbeddingModelTip = ref(false);
const showHfEmbeddingModelDownloadProgress = ref(false);
const sentenceEmbeddingModelRepository = ref("");
const originalSentenceEmbeddingModelId = ref("");
const downloadingUrl = ref("");
const downloadingProgress = ref("");

onMounted(async () => {
    const r = await httpReq(
        "GET",
        "management/settings",
        { robotId: robotId },
        null,
        null,
    );
    if (r.status == 200) {
        copyProperties(r.data, settings);
        maxSessionIdleMin.value = settings.maxSessionIdleSec / 60;
        if (settings.sentenceEmbeddingProvider.similarityThreshold != null)
            similarityThreshold.value = Math.round(
                settings.sentenceEmbeddingProvider.similarityThreshold * 100,
            );
        originalSentenceEmbeddingModelId.value =
            r.data.sentenceEmbeddingProvider.provider.id;
        // 这件事必须在 change*Provider 之前做：
        // 把已存地址种进 urlMap。change*Provider 的 else 分支要读这个 map，
        // 不种的话它会拿到 undefined，于是把刚 copyProperties 进来的用户
        // 地址覆盖成预设值——配了远程地址的用户进一次设置页再保存就被改回去了。
        //
        // 这里**不做**空地址兜底：每条记录都是建机器人时由 settings::init() 写下的
        // Settings::default()，那时的 provider 是 HuggingFace、api_url 是空串，
        // 而 HuggingFace 那条分支会把地址显示替换掉。所以"在线模型 + 空地址"
        // 只可能来自用户自己清空——把它悄悄换成 OpenAI 的地址，正是后端刚
        // 去掉的那个行为。空地址会由后端明确报错，这才是我们想要的。
        chatDynamicReqUrlMap.set(
            settings.chatProvider.provider.id,
            settings.chatProvider.apiUrl,
        );
        sentenceEmbeddingDynamicReqUrlMap.set(
            settings.sentenceEmbeddingProvider.provider.id,
            settings.sentenceEmbeddingProvider.apiUrl,
        );
        await changeChatProvider(settings.chatProvider.provider.id);
        await changeSentenceEmbeddingProvider(
            settings.sentenceEmbeddingProvider.provider.id,
        );
    }
    await checkHfModelFiles();
});
onUnmounted(() => {
    if (timeoutID != null) clearTimeout(timeoutID);
});

async function checkHfModelFiles() {
    const repostories = new Map();
    if (settings.chatProvider.provider.id == "HuggingFace") {
        for (let i = 0; i < chatModelOptions.length; i++) {
            if (
                chatModelOptions[i].value ==
                settings.chatProvider.provider.model
            ) {
                let l = chatModelOptions[i].value;
                const p = l.lastIndexOf(" ");
                if (p > -1) l = l.substring(0, p);
                chatModelRepository.value = l;
                repostories.set(showHfIncorrectChatModelTip, l);
                break;
            }
        }
    } else showHfIncorrectChatModelTip.value = false;
    if (settings.sentenceEmbeddingProvider.provider.id == "HuggingFace") {
        for (let i = 0; i < sentenceEmbeddingModelOptions.length; i++) {
            if (
                sentenceEmbeddingModelOptions[i].value ==
                settings.sentenceEmbeddingProvider.provider.model
            ) {
                let l = sentenceEmbeddingModelOptions[i].value;
                const p = l.lastIndexOf(" ");
                if (p > -1) l = l.substring(0, p);
                sentenceEmbeddingModelRepository.value = l;
                repostories.set(showHfIncorrectEmbeddingModelTip, l);
                break;
            }
        }
    } else showHfIncorrectEmbeddingModelTip.value = false;
    if (repostories.size > 0) {
        const r = await httpReq(
            "POST",
            "management/settings/model/check/files",
            null,
            null,
            Array.from(repostories.values()),
        );
        if (r && r.data) {
            for (let [k, v] of repostories.entries()) {
                if (r.data[v] == false) {
                    k.value = true;
                } else k.value = false;
            }
        }
    }
}

async function save() {
    if (
        originalSentenceEmbeddingModelId.value !=
        settings.sentenceEmbeddingProvider.provider.id
    ) {
        ElMessageBox.confirm(
            t("botSettings.modelChangedWarning"),
            t("common.warning"),
            {
                confirmButtonText: t("common.confirm"),
                cancelButtonText: t("common.cancel"),
                type: "warning",
                dangerouslyUseHTMLString: true,
            },
        )
            .then(() => {
                saveSettings();
            })
            .catch(() => {});
    } else saveSettings();
}

async function saveSettings() {
    if (!settings.emailVerificationRegex)
        settings.emailVerificationRegex = defaultEmailVerificationRegex;
    settings.maxSessionIdleSec = maxSessionIdleMin.value * 60;
    settings.sentenceEmbeddingProvider.similarityThreshold =
        similarityThreshold.value / 100;
    const r = await httpReq(
        "POST",
        "management/settings",
        { robotId: robotId },
        null,
        settings,
    );
    if (r.status == 200) {
        ElMessage({ type: "success", message: t("common.saved") });
        await checkHfModelFiles();
    } else {
        const m = t(r.err.message);
        ElMessage.error(m ? m : r.err.message);
    }
}

let timeoutID = null;

async function downloadModels(m) {
    const r = await httpReq(
        "GET",
        "management/settings/model/download/progress",
        null,
        null,
        null,
    );
    if (r != null && r.data != null && r.data.downloading) {
        const msg =
            "Downloading: " +
            r.data.url +
            " (" +
            ((r.data.downloadedLen / r.data.totalLen) * 100).toFixed(2) +
            "%), please wait until it finish.";
        ElMessage.error(msg);
        return;
    }
    httpReq(
        "POST",
        "management/settings/model/download",
        { robotId: robotId, m: m },
        null,
        m,
    ).then((r) => {
        if (r == null || r.status != 200) {
            ElMessage.error("Download failed: " + r.err.message);
            return;
        }
        if (m == "sentenceEmbedding") {
            showHfIncorrectEmbeddingModelTip.value = false;
            showHfEmbeddingModelDownloadProgress.value = true;
        } else {
            // 本地对话模型的下载。进度以前是写进"文本生成"那一块的 ref（那个
            // 区块已经并掉了），而对话区块自己绑的 showHfChatModelDownloadProgress
            // 从来没人赋值——于是下载中一个进度条都不显示。现在写它自己这个。
            showHfIncorrectChatModelTip.value = false;
            showHfChatModelDownloadProgress.value = true;
        }
        timeoutID = setTimeout(async () => {
            await showDownloadProgress();
        }, 1000);
    });
}

function downloadComplete() {
    clearTimeout(timeoutID);
    showHfIncorrectChatModelTip.value = false;
    showHfChatModelDownloadProgress.value = false;
    showHfIncorrectEmbeddingModelTip.value = false;
    showHfEmbeddingModelDownloadProgress.value = false;
}

async function showDownloadProgress() {
    const r = await httpReq(
        "GET",
        "management/settings/model/download/progress",
        null,
        null,
        null,
    );
    if (r != null && r.data != null) {
        if (r.data.err) {
            ElMessage.error(r.data.err);
            clearTimeout(timeoutID);
            showHfChatModelDownloadProgress.value = false;
            showHfEmbeddingModelDownloadProgress.value = false;
            return;
        } else if (r.data.downloading) {
            downloadingUrl.value = r.data.url;
            downloadingProgress.value = (
                (r.data.downloadedLen / r.data.totalLen) *
                100
            ).toFixed(2);
            timeoutID = setTimeout(async () => {
                await showDownloadProgress();
            }, 1000);
        } else downloadComplete();
    } else {
        downloadComplete();
    }
}

const smtpTest = async () => {
    loading.value = true;
    const r = await httpReq(
        "POST",
        "management/settings/smtp/test",
        null,
        null,
        settings,
    );
    if (r.status == 200) {
        smtpPassed.value = true;
        smtpFailed.value = false;
    } else {
        const m = t(r.err.message);
        smtpFailedDetail.value = m ? m : r.err.message;
        smtpPassed.value = false;
        smtpFailed.value = true;
    }
    loading.value = false;
};

const ollamaModels = [];
provide("ollamaModels", { ollamaModels });

// OpenAI 兼容端点的厂商预设。
//
// 厂商不做持久化，而是每次从 apiUrl 反查出来。本次改动里 URL 是唯一决定
// 行为的因素，所以反查结果不可能与实际请求不一致；代价是同一网关下的不同
// 厂商会被归成"自定义"，可以接受——预设的职责只是填 URL 和给模型候选。
//
// apiUrl / embedUrl 必须是**完整的端点路径**（形如 .../v1/chat/completions），
// 不是 base URL：后端就是拿它直接 POST 的。厂商模型名会变，用之前对一下。
const compatibleVendors = [
    {
        key: "openai",
        nameKey: "botSettings.vendorOpenAi",
        chatUrl: "https://api.openai.com/v1/chat/completions",
        chatModels: [
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4-vision-preview",
            "gpt-3.5-turbo",
        ],
        embedUrl: "https://api.openai.com/v1/embeddings",
        embedModels: [
            "text-embedding-3-large",
            "text-embedding-3-small",
            "text-embedding-ada-002",
        ],
    },
    {
        key: "deepseek",
        nameKey: "botSettings.vendorDeepSeek",
        chatUrl: "https://api.deepseek.com/v1/chat/completions",
        chatModels: ["deepseek-chat", "deepseek-reasoner"],
    },
    {
        key: "zhipu",
        nameKey: "botSettings.vendorZhipu",
        chatUrl: "https://open.bigmodel.cn/api/paas/v4/chat/completions",
        chatModels: ["glm-4-plus", "glm-4-air", "glm-4-flash"],
        embedUrl: "https://open.bigmodel.cn/api/paas/v4/embeddings",
        embedModels: ["embedding-3", "embedding-2"],
    },
    {
        key: "qwen",
        nameKey: "botSettings.vendorQwen",
        chatUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
        chatModels: ["qwen-max", "qwen-plus", "qwen-turbo"],
        embedUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1/embeddings",
        embedModels: ["text-embedding-v3", "text-embedding-v2"],
    },
    {
        key: "moonshot",
        nameKey: "botSettings.vendorMoonshot",
        chatUrl: "https://api.moonshot.cn/v1/chat/completions",
        chatModels: ["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"],
    },
    // MiniMax 的 OpenAI 兼容端点在 api.minimax.io（国际版）/ api.minimaxi.com
    // （中国大陆版，注意域名末尾多一个 i），两者按账号区域划分，密钥不通用。
    // 这里填国际版；国内账号把域名换成 api.minimaxi.com 即可，路径不变。
    //
    // 只有对话端点，**没有 embeddings**：MiniMax 没有 OpenAI 兼容的向量接口
    // （它的 embedding 走原生 /v1/embeddings，请求体是 texts 而不是 input）。
    // 于是这里不写 embedUrl，向量区块的厂商列表里自然就不会出现 MiniMax，
    // 与 moonshot / groq / openrouter 一致。
    {
        key: "minimax",
        nameKey: "botSettings.vendorMinimax",
        chatUrl: "https://api.minimax.io/v1/chat/completions",
        chatModels: [
            "MiniMax-M3",
            "MiniMax-M2.7",
            "MiniMax-M2.7-highspeed",
            "MiniMax-M2.5",
            "MiniMax-M2.5-highspeed",
            "MiniMax-M2.1",
            "MiniMax-M2.1-highspeed",
            "MiniMax-M2",
        ],
    },
    {
        key: "siliconflow",
        nameKey: "botSettings.vendorSiliconFlow",
        chatUrl: "https://api.siliconflow.cn/v1/chat/completions",
        chatModels: ["deepseek-ai/DeepSeek-V3", "Qwen/Qwen2.5-7B-Instruct"],
        embedUrl: "https://api.siliconflow.cn/v1/embeddings",
        embedModels: ["BAAI/bge-m3", "BAAI/bge-large-zh-v1.5"],
    },
    {
        key: "groq",
        nameKey: "botSettings.vendorGroq",
        chatUrl: "https://api.groq.com/openai/v1/chat/completions",
        chatModels: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
    },
    {
        key: "openrouter",
        nameKey: "botSettings.vendorOpenRouter",
        chatUrl: "https://openrouter.ai/api/v1/chat/completions",
        chatModels: ["openai/gpt-4o-mini", "anthropic/claude-3.5-sonnet"],
    },
    // Ollama 从 v0.1.24 起提供 OpenAI 兼容端点，官方文档明确支持流式、`max_tokens`
    // 和 base64 图片（`image_url` 的字符串和对象两种写法都收），所以我们走统一的
    // `/v1/chat/completions`，不再单开一条原生 `/api/chat` 的路径。
    // 向量同理走 `/v1/embeddings`。
    {
        key: "ollama",
        nameKey: "botSettings.vendorOllama",
        chatUrl: "http://localhost:11434/v1/chat/completions",
        chatModels: [],
        embedUrl: "http://localhost:11434/v1/embeddings",
        embedModels: ["nomic-embed-text", "bge-m3", "mxbai-embed-large"],
    },
    {
        key: "vllm",
        nameKey: "botSettings.vendorVllm",
        chatUrl: "http://localhost:8000/v1/chat/completions",
        chatModels: [],
        embedUrl: "http://localhost:8000/v1/embeddings",
        embedModels: [],
    },
    {
        key: "lmstudio",
        nameKey: "botSettings.vendorLmStudio",
        chatUrl: "http://localhost:1234/v1/chat/completions",
        chatModels: [],
        embedUrl: "http://localhost:1234/v1/embeddings",
        embedModels: [],
    },
    // 留空的地址，让用户自己填。
    {
        key: "custom",
        nameKey: "botSettings.vendorCustom",
        chatUrl: "",
        chatModels: [],
        embedUrl: "",
        embedModels: [],
    },
];
const customVendor = compatibleVendors[compatibleVendors.length - 1];

// 只归一化尾斜杠和大小写，**不做前缀匹配**：用户通过网关/代理走的路径是
// 合法的，前缀匹配会把用户自己填的地址重新贴成别家的标签。
const normalizeUrl = (u) => (u || "").trim().replace(/\/+$/, "").toLowerCase();
const vendorUrlKey = (kind) => (kind == "embedding" ? "embedUrl" : "chatUrl");
const vendorUrl = (v, kind) => v[vendorUrlKey(kind)] || "";
// 走这个而不是直接写 v.chatModels / v.embedModels：向量区块复制对话区块的
// 代码时，最后一个 chatModels 忘了改是很容易发生的事。
const vendorModels = (v, kind) =>
    (kind == "embedding" ? v.embedModels : v.chatModels) || [];
// 某区块可选的厂商。没有 embeddings 端点的（moonshot / groq / openrouter）
// 不出现在向量区块的列表里。
const vendorsFor = (kind) =>
    compatibleVendors.filter((v) => v.key == "custom" || vendorUrl(v, kind));
const vendorByKey = (key) =>
    compatibleVendors.find((v) => v.key == key) || customVendor;
// 从地址反查厂商；认不出来归到"自定义"。空地址返回空串，交给调用方兜底。
const deriveVendorKey = (apiUrl, kind) => {
    const u = normalizeUrl(apiUrl);
    if (!u) return "";
    const k = vendorUrlKey(kind);
    const hit = compatibleVendors.find((v) => v[k] && normalizeUrl(v[k]) == u);
    return hit ? hit.key : "custom";
};
// 选中「自定义」时调用：把地址框里的**厂商预设地址**清掉。
//
// 为什么必须清：下拉的值不是用户状态，而是每次从地址反查出来的（deriveVendorKey）。
// 不清的话 refresh*Vendor 会拿框里那个旧厂商的地址回查，把下拉改回旧厂商——表现就是
// "选自定义完全没反应"（「自定义」没有预设地址，else 分支什么都不写，于是反查结果必是
// 旧厂商）。清掉之后反查得到空串，归属自然是「自定义」，下拉显示、模型候选、保存后
// 重新加载三者才一致。
//
// 为什么可以清：onMounted 在 change*Provider 之前就把已存地址种进了 map，空地址在
// map 里是 "" 而不是 undefined，重载时走 `u != null` 分支原样保留，不会被
// OpenAICompatible 的预设地址覆盖回来（详见 onMounted 里那段注释）。
//
// 为什么只清预设：反查为空串说明本来就是「自定义」，没什么可清；反查为 custom 说明
// 那是用户自己手输的地址，绝不能动。只有"再点一下另一边就能选回来的厂商预设"才清。
const clearPresetApiUrl = (provider, kind) => {
    const derived = deriveVendorKey(provider.apiUrl, kind);
    if (derived && derived != "custom") provider.apiUrl = "";
};
// 值非空且不在候选里就补一个选项。**必要**，不是可选：Element Plus 关闭态
// 显示的是匹配到的 option 的 label，缺了它，重载后的自定义模型名会显示成
// 一个空白框（值其实是对的）。
//
// 代价：切换厂商后，上一个厂商的模型名会作为候选留在列表里（因为它是当前
// 选中值）。没法区分"用户手输的名字"和"上个厂商的预设名"——两者在存储里
// 都是同一个字符串。留着比丢掉好：丢掉会让当前选择显示成空白框。真选了
// 不对的模型，后端会带着端点地址报错。
const ensureOption = (list, value) => {
    if (!value) return;
    if (list.find((d) => d.value == value) == null) list.unshift({ label: value, value });
};

// https://docs.spring.io/spring-ai/reference/api/chat/completions.html
const chatProviders = [
    {
        id: "HuggingFace",
        nameKey: "botSettings.providerHuggingFace",
        apiUrl: "Model will be downloaded locally at ./data/models",
        apiUrlDisabled: true,
        showApiKeyInput: false,
        models: [
            {
                label: "microsoft/Phi-3-mini-4k-instruct (7.7GB)",
                value: "Phi3Mini4kInstruct",
            },
            {
                label: "microsoft/Phi-3-mini-128k-instruct (7.7GB)",
                value: "Phi3Mini128kInstruct",
            },
            {
                label: "microsoft/Phi-3-small-8k-instruct (15GB)",
                value: "Phi3Small8kInstruct",
            },
            {
                label: "microsoft/Phi-3-small-128k-instruct (15GB)",
                value: "Phi3Small128kInstruct",
            },
            {
                label: "microsoft/Phi-3-medium-4k-instruct (30GB)",
                value: "Phi3Medium4kInstruct",
            },
            {
                label: "microsoft/Phi-3-medium-128k-instruct (30GB)",
                value: "Phi3Medium128kInstruct",
            },
            {
                label: "google/gemma-2b-it (4.9GB)",
                value: "Gemma2bInstruct",
                need_auth_header: true,
            },
            {
                label: "google/gemma-7b-it (12.1GB)",
                value: "Gemma7bInstruct",
                need_auth_header: true,
            },
            {
                label: "meta-llama/Meta-Llama-3-8B-Instruct (??GB)",
                value: "MetaLlama3_8bInstruct",
                need_auth_header: true,
            },
            {
                label: "upstage/SOLAR-10.7B-v1.0 (21.5GB)",
                value: "Solar10_7bV1_0",
            },
            {
                label: "Qwen/Qwen2-7B-Instruct (15.4GB)",
                value: "Qwen2_7BInstruct",
            },
            {
                label: "Qwen/Qwen2-72B-Instruct (144GB)",
                value: "Qwen2_72BInstruct",
            },
            {
                label: "TinyLlama/TinyLlama-1.1B-Chat-v1.0 (2.2GB)",
                value: "TinyLlama1_1bChatV1_0",
            },
        ],
    },
    {
        id: "OpenAICompatible",
        nameKey: "botSettings.providerOpenAiCompatible",
        // 地址只是没配过时的兜底，真正的值来自 onMounted 里种进 map 的已存
        // 记录；changeChatProvider 会拿厂商预设覆盖模型候选。
        apiUrl: "https://api.openai.com/v1/chat/completions",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        models: [],
    },
];

const chatProviderProxyEnabled = ref(false);
const isAddingAnotherChatModel = ref(false);
const anotherChatModel = ref("");
const chatModelSelector = ref();
// 「添加自定义模型名」。原来这里硬编码 chatProviders[2]（数组一旦少一项就
// 静默加错地方），还无条件把 provider.id 改成 "Ollama"。现在加到用户当前
// 选中的那一项，且不再改 provider——那是单选按钮的职责。
const addAnotherChatModel = (m) => {
    if (!m || choosedChatProvider.value == "HuggingFace") return;
    const p = chatProviders.find((d) => d.id == choosedChatProvider.value);
    if (p == null) return;
    ensureOption(chatModelOptions, m);
    ensureOption(p.models, m);
    chatModelSelector.value?.blur();
    settings.chatProvider.provider.model = m;
    anotherChatModel.value = "";
};

// https://docs.spring.io/spring-ai/reference/api/embeddings.html
const sentenceEmbeddingProviders = [
    {
        id: "HuggingFace",
        nameKey: "botSettings.providerHuggingFace",
        apiUrl: "Model will be downloaded locally at ./data/models",
        apiUrlDisabled: true,
        showApiKeyInput: false,
        models: [
            {
                label: "sentence-transformers/all-MiniLM-L6-v2 (91MB)",
                value: "AllMiniLML6V2",
            },
            {
                label: "sentence-transformers/paraphrase-MiniLM-L12-v2 (135MB)",
                value: "ParaphraseMLMiniLML12V2",
            },
            {
                label: "sentence-transformers/paraphrase-multilingual-mpnet-base-v2 (1.11GB)",
                value: "ParaphraseMLMpnetBaseV2",
            },
            {
                label: "BAAI/bge-small-en-v1.5 (135MB)",
                value: "BgeSmallEnV1_5",
            },
            { label: "BAAI/bge-base-en-v1.5 (439MB)", value: "BgeBaseEnV1_5" },
            {
                label: "BAAI/bge-large-en-v1.5 (1.35GB)",
                value: "BgeLargeEnV1_5",
            },
            { label: "BAAI/bge-m3 (2.27GB)", value: "BgeM3" },
            {
                label: "nomic-ai/nomic-embed-text-v1.5 (550MB)",
                value: "NomicEmbedTextV1_5",
            },
            {
                label: "intfloat/multilingual-e5-small (472MB)",
                value: "MultilingualE5Small",
            },
            {
                label: "intfloat/multilingual-e5-base (1.11GB)",
                value: "MultilingualE5Base",
            },
            {
                label: "intfloat/multilingual-e5-large (2.24GB)",
                value: "MultilingualE5Large",
            },
            {
                label: "mixedbread-ai/mxbai-embed-large-v1 (1.34GB)",
                value: "MxbaiEmbedLargeV1",
            },
            { label: "moka-ai/m3e-base (409MB)", value: "MokaAiM3eBase" },
            { label: "moka-ai/m3e-large (1.3GB)", value: "MokaAiM3eLarge" },
        ],
    },
    {
        id: "OpenAICompatible",
        nameKey: "botSettings.providerOpenAiCompatible",
        apiUrl: "https://api.openai.com/v1/embeddings",
        apiUrlDisabled: false,
        showApiKeyInput: true,
        models: [],
    },
];
const sentenceEmbeddingProviderProxyEnabled = ref(false);
const chatModelOptions = reactive([]);
const chatDynamicReqUrlMap = new Map();
const choosedChatProvider = ref("");
const chatVendorKey = ref("");
// 从当前地址反查厂商写回下拉，并用该厂商的模型候选重建候选列表。
// 两个触发点：选中「在线模型」，以及用户手改地址后失焦（@change，不是逐键）。
const refreshChatVendor = () => {
    const p = chatProviders.find((d) => d.id == "OpenAICompatible");
    const v = vendorByKey(deriveVendorKey(settings.chatProvider.apiUrl, "chat"));
    chatVendorKey.value = v.key;
    p.models.splice(
        0,
        p.models.length,
        ...vendorModels(v, "chat").map((m) => ({ label: m, value: m })),
    );
    ensureOption(p.models, settings.chatProvider.provider.model);
    chatModelOptions.splice(0, chatModelOptions.length, ...p.models);
};
// 选厂商 = 填它的地址（没有预设的「自定义」则清掉预设地址），然后照常刷新。
const applyChatVendorPreset = (key) => {
    const u = vendorUrl(vendorByKey(key), "chat");
    if (u) settings.chatProvider.apiUrl = u;
    else if (key == "custom") clearPresetApiUrl(settings.chatProvider, "chat");
    refreshChatVendor();
};
const changeChatProvider = async (n) => {
    if (choosedChatProvider.value)
        chatDynamicReqUrlMap.set(
            choosedChatProvider.value,
            settings.chatProvider.apiUrl,
        );
    for (let i = 0; i < chatProviders.length; i++) {
        if (chatProviders[i].id == n) {
            if (chatProviders[i].apiUrlDisabled)
                settings.chatProvider.apiUrl = chatProviders[i].apiUrl;
            else {
                // 判断"在不在 map 里"，不是"值真不真"：用户把地址清空后保存，
                // 存下来的就是空串，那是他的选择，不能拿预设把它填回去。
                // map 里没有这一项才说明这个 provider 是第一次被选中。
                const u = chatDynamicReqUrlMap.get(n);
                settings.chatProvider.apiUrl =
                    u != null ? u : chatProviders[i].apiUrl;
            }
            settings.chatProvider.apiUrlDisabled =
                chatProviders[i].apiUrlDisabled;
            settings.chatProvider.showApiKeyInput =
                chatProviders[i].showApiKeyInput;
            choosedChatProvider.value = n;
            // 关掉可能开着的「添加自定义模型名」表单：它的输入框不跟着 provider
            // 变，留着会让用户在切到本地模型后提交一个 HuggingFace 不认的模型名。
            isAddingAnotherChatModel.value = false;
            // 不在这里拉模型列表：加载时也会走到这里，那等于每次进设置页都
            // 打向（可能是内网、可能连不通的）用户端点。拉取只在按钮后面。
            if (n == "OpenAICompatible") refreshChatVendor();
            else
                chatModelOptions.splice(
                    0,
                    chatModelOptions.length,
                    ...chatProviders[i].models,
                );
            break;
        }
    }
    // 存量记录里可能存着已从选择器移除的 provider（本轮的 Ollama）。这里刻意
    // **不动记录本身**——后端仍认得它、原生路径照走，直接保存也不会丢——只是
    // 单选按钮没法高亮它。至少把已存的模型名显示出来，别是一片空白。
    if (!choosedChatProvider.value)
        ensureOption(chatModelOptions, settings.chatProvider.provider.model);
};
const chatModelListLoading = ref(false);
const fetchChatModelList = async () => {
    if (!settings.chatProvider.apiUrl) return;
    chatModelListLoading.value = true;
    try {
        const r = await httpReq(
            "GET",
            "management/settings/model/openai/list",
            {
                url: settings.chatProvider.apiUrl,
                apiKey: settings.chatProvider.apiKey,
                proxyUrl: settings.chatProvider.proxyUrl,
                connectTimeoutMillis: settings.chatProvider.connectTimeoutMillis,
                readTimeoutMillis: settings.chatProvider.readTimeoutMillis,
            },
            null,
            null,
        );
        if (r.status != 200 || !Array.isArray(r.data))
            throw new Error(r.err?.message || "bad response");
        // 端点返回的就是权威列表，直接替换掉厂商预设的那批候选。
        const p = chatProviders.find((d) => d.id == "OpenAICompatible");
        p.models.splice(
            0,
            p.models.length,
            ...r.data.map((m) => ({ label: m, value: m })),
        );
        ensureOption(p.models, settings.chatProvider.provider.model);
        chatModelOptions.splice(0, chatModelOptions.length, ...p.models);
        ElMessage.success(
            t("botSettings.fetchModelListOk", { count: r.data.length }),
        );
    } catch {
        console.error("拉取模型列表失败", r.err?.message || "bad response");
        // 只提示失败，**不动**用户已经手输的模型名：拉取不到的端点照样能用。
        ElMessage.error(t("botSettings.fetchModelListFailed"));
    } finally {
        chatModelListLoading.value = false;
    }
};

const sentenceEmbeddingModelOptions = reactive([]);
const sentenceEmbeddingDynamicReqUrlMap = new Map();
const choosedSentenceEmbeddingProvider = ref("");
const sentenceEmbeddingVendorKey = ref("");
const refreshSentenceEmbeddingVendor = () => {
    const p = sentenceEmbeddingProviders.find((d) => d.id == "OpenAICompatible");
    const v = vendorByKey(
        deriveVendorKey(settings.sentenceEmbeddingProvider.apiUrl, "embedding"),
    );
    sentenceEmbeddingVendorKey.value = v.key;
    p.models.splice(
        0,
        p.models.length,
        ...vendorModels(v, "embedding").map((m) => ({ label: m, value: m })),
    );
    ensureOption(p.models, settings.sentenceEmbeddingProvider.provider.model);
    sentenceEmbeddingModelOptions.splice(
        0,
        sentenceEmbeddingModelOptions.length,
        ...p.models,
    );
};
const applySentenceEmbeddingVendorPreset = (key) => {
    // 同 chat：没有预设的「自定义」清掉厂商预设地址，理由和边界见 clearPresetApiUrl。
    const u = vendorUrl(vendorByKey(key), "embedding");
    if (u) settings.sentenceEmbeddingProvider.apiUrl = u;
    else if (key == "custom")
        clearPresetApiUrl(settings.sentenceEmbeddingProvider, "embedding");
    refreshSentenceEmbeddingVendor();
};
const changeSentenceEmbeddingProvider = async (n) => {
    if (choosedSentenceEmbeddingProvider.value)
        sentenceEmbeddingDynamicReqUrlMap.set(
            choosedSentenceEmbeddingProvider.value,
            settings.sentenceEmbeddingProvider.apiUrl,
        );
    for (let i = 0; i < sentenceEmbeddingProviders.length; i++) {
        if (sentenceEmbeddingProviders[i].id == n) {
            if (sentenceEmbeddingProviders[i].apiUrlDisabled)
                settings.sentenceEmbeddingProvider.apiUrl =
                    sentenceEmbeddingProviders[i].apiUrl;
            else {
                // 同 chat：看"在不在 map 里"，不看值真不真。
                const u = sentenceEmbeddingDynamicReqUrlMap.get(n);
                settings.sentenceEmbeddingProvider.apiUrl =
                    u != null ? u : sentenceEmbeddingProviders[i].apiUrl;
            }
            settings.sentenceEmbeddingProvider.apiUrlDisabled =
                sentenceEmbeddingProviders[i].apiUrlDisabled;
            settings.sentenceEmbeddingProvider.showApiKeyInput =
                sentenceEmbeddingProviders[i].showApiKeyInput;
            choosedSentenceEmbeddingProvider.value = n;
            // 同 chat：关掉可能开着的添加表单。
            isAddingAnotherSentenceEmbeddingModel.value = false;
            // 同 chat：不在加载路径上打用户的端点，拉取只在按钮后面。
            if (n == "OpenAICompatible") refreshSentenceEmbeddingVendor();
            else
                sentenceEmbeddingModelOptions.splice(
                    0,
                    sentenceEmbeddingModelOptions.length,
                    ...sentenceEmbeddingProviders[i].models,
                );
            break;
        }
    }
    // 同 chat：存量的、已从选择器移除的 provider 只保证模型名可见。
    if (!choosedSentenceEmbeddingProvider.value)
        ensureOption(
            sentenceEmbeddingModelOptions,
            settings.sentenceEmbeddingProvider.provider.model,
        );
};
const sentenceEmbeddingModelListLoading = ref(false);
const fetchSentenceEmbeddingModelList = async () => {
    if (!settings.sentenceEmbeddingProvider.apiUrl) return;
    sentenceEmbeddingModelListLoading.value = true;
    try {
        const r = await httpReq(
            "GET",
            "management/settings/model/openai/list",
            {
                url: settings.sentenceEmbeddingProvider.apiUrl,
                apiKey: settings.sentenceEmbeddingProvider.apiKey,
                proxyUrl: settings.sentenceEmbeddingProvider.proxyUrl,
                connectTimeoutMillis:
                    settings.sentenceEmbeddingProvider.connectTimeoutMillis,
                readTimeoutMillis:
                    settings.sentenceEmbeddingProvider.readTimeoutMillis,
            },
            null,
            null,
        );
        if (r.status != 200 || !Array.isArray(r.data))
            throw new Error(r.err?.message || "bad response");
        // 端点返回的就是权威列表，直接替换掉厂商预设的那批候选。
        const p = sentenceEmbeddingProviders.find(
            (d) => d.id == "OpenAICompatible",
        );
        p.models.splice(
            0,
            p.models.length,
            ...r.data.map((m) => ({ label: m, value: m })),
        );
        ensureOption(p.models, settings.sentenceEmbeddingProvider.provider.model);
        sentenceEmbeddingModelOptions.splice(
            0,
            sentenceEmbeddingModelOptions.length,
            ...p.models,
        );
        ElMessage.success(
            t("botSettings.fetchModelListOk", { count: r.data.length }),
        );
    } catch {
        ElMessage.error(t("botSettings.fetchModelListFailed"));
    } finally {
        sentenceEmbeddingModelListLoading.value = false;
    }
};

const sentenceEmbeddingModelSelector = ref();
const isAddingAnotherSentenceEmbeddingModel = ref(false);
const anotherSentenceEmbeddingModel = ref("");
const addAnotherSentenceEmbeddingModel = (m) => {
    if (!m || choosedSentenceEmbeddingProvider.value == "HuggingFace") return;
    const p = sentenceEmbeddingProviders.find(
        (d) => d.id == choosedSentenceEmbeddingProvider.value,
    );
    if (p == null) return;
    ensureOption(sentenceEmbeddingModelOptions, m);
    ensureOption(p.models, m);
    sentenceEmbeddingModelSelector.value?.blur();
    settings.sentenceEmbeddingProvider.provider.model = m;
    anotherSentenceEmbeddingModel.value = "";
};

const usedByLlmChatNodeBig = [chatPic];
const usedBySentenceEmbeddingBig = [sentenceEmbeddingPic];
</script>
<template>
    <div class="page-header">
        <h1 class="page-title">{{ $t("settings.title") }}</h1>
        <el-button @click="goBack()">{{ $t("common.back") }}</el-button>
    </div>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">{{ $t("settings.commonSettings") }}</div>
        </template>
        <el-form :model="settings" :label-width="formLabelWidth">
            <el-form-item :label="$t('botSettings.prompt3')">
                <el-input-number
                    v-model="maxSessionIdleMin"
                    :min="2"
                    :max="1440"
                />
                <span class="form-item-suffix">{{
                    $t("botSettings.prompt4")
                }}</span>
            </el-form-item>
        </el-form>
    </el-card>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">
                {{ $t("botSettings.chatModel") }}
                <el-tooltip effect="light" placement="right">
                    <template #content>
                        <span v-html="$t('botSettings.chatModelTip')"></span>
                    </template>
                    <el-button circle>?</el-button>
                </el-tooltip>
            </div>
        </template>
        <el-row>
            <el-col :span="16">
                <el-form
                    :model="settings.chatProvider"
                    :label-width="formLabelWidth"
                >
                    <el-form-item :label="t('botSettings.provider')">
                        <el-radio-group
                            v-model="settings.chatProvider.provider.id"
                            @change="changeChatProvider"
                        >
                            <el-radio-button
                                v-for="item in chatProviders"
                                :id="item.id"
                                :key="item.id"
                                :label="item.id"
                                :value="item.id"
                            >
                                {{ $t(item.nameKey) }}
                            </el-radio-button>
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item
                        v-if="
                            settings.chatProvider.provider.id ==
                            'OpenAICompatible'
                        "
                        :label="t('botSettings.vendor')"
                    >
                        <el-select
                            v-model="chatVendorKey"
                            @change="applyChatVendorPreset"
                        >
                            <el-option
                                v-for="item in vendorsFor('chat')"
                                :key="item.key"
                                :label="$t(item.nameKey)"
                                :value="item.key"
                            />
                        </el-select>
                        <div class="form-item-help">
                            {{ $t("botSettings.compatibleApiHint") }}
                        </div>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="settings.chatProvider.apiUrl"
                            :disabled="settings.chatProvider.apiUrlDisabled"
                            @change="refreshChatVendor"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="$t('botSettings.apiKey')"
                        v-show="settings.chatProvider.showApiKeyInput"
                    >
                        <el-input
                            v-model="settings.chatProvider.apiKey"
                            show-password
                        />
                    </el-form-item>
                    <el-form-item :label="t('botSettings.model')">
                        <el-select
                            ref="chatModelSelector"
                            v-model="settings.chatProvider.provider.model"
                            :placeholder="$t('botSettings.chooseModel')"
                            :filterable="
                                settings.chatProvider.provider.id !=
                                'HuggingFace'
                            "
                            :allow-create="
                                settings.chatProvider.provider.id !=
                                'HuggingFace'
                            "
                        >
                            <el-option
                                v-for="item in chatModelOptions"
                                :id="item.value"
                                :key="item.value"
                                :label="item.label"
                                :value="item.value"
                            />
                            <template #footer>
                                <el-button
                                    :disabled="
                                        settings.chatProvider.provider.id ==
                                        'HuggingFace'
                                    "
                                    v-if="!isAddingAnotherChatModel"
                                    text
                                    bg
                                    @click="isAddingAnotherChatModel = true"
                                >
                                    {{ $t("botSettings.anotherModel") }}
                                </el-button>
                                <!-- 表单本身也要挡住 HuggingFace：只禁用按钮
                                     不够，表单一旦开着，切到本地模型后它仍在
                                     渲染，提交进去的模型名会让整个设置保存
                                     被 serde 拒绝。 -->
                                <template
                                    v-else-if="
                                        settings.chatProvider.provider.id !=
                                        'HuggingFace'
                                    "
                                >
                                    <el-input
                                        v-model="anotherChatModel"
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherChatModel(anotherChatModel)
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="isAddingAnotherChatModel = false"
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
                        <el-button
                            v-if="
                                settings.chatProvider.provider.id ==
                                'OpenAICompatible'
                            "
                            :loading="chatModelListLoading"
                            :disabled="!settings.chatProvider.apiUrl"
                            style="margin-left: 8px"
                            @click="fetchChatModelList"
                        >
                            {{ $t("botSettings.fetchModelList") }}
                        </el-button>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.maxResTokenLen')">
                        <el-input-number
                            v-model="
                                settings.chatProvider.maxResponseTokenLength
                            "
                            :min="10"
                            :max="100000"
                            :step="5"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.connTimeout')"
                        v-show="
                            settings.chatProvider.provider.id != 'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="settings.chatProvider.connectTimeoutMillis"
                            :min="100"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.readTimeout')"
                        v-show="
                            settings.chatProvider.provider.id != 'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="settings.chatProvider.readTimeoutMillis"
                            :min="200"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.proxy')"
                        v-show="
                            settings.chatProvider.provider.id != 'HuggingFace'
                        "
                    >
                        <div class="proxy-row">
                            <el-switch
                                v-model="chatProviderProxyEnabled"
                                :active-text="$t('common.enable')"
                            />
                            <el-input
                                v-model="settings.chatProvider.proxyUrl"
                                placeholder="http://127.0.0.1:9270"
                                :disabled="!chatProviderProxyEnabled"
                            />
                        </div>
                    </el-form-item>
                </el-form>
            </el-col>
            <el-col :span="7" :offset="1">
                <div class="usage-note">
                    {{ $t("botSettings.chatModelUsage") }}
                </div>
                <el-image
                    :src="chatPicThumbnail"
                    :zoom-rate="1.2"
                    :max-scale="7"
                    :min-scale="0.2"
                    :preview-src-list="usedByLlmChatNodeBig"
                    :initial-index="4"
                    fit="cover"
                />
            </el-col>
        </el-row>
        <el-alert
            v-if="showHfIncorrectChatModelTip"
            type="warning"
            :closable="false"
            class="hf-alert"
        >
            <template #title>
                {{ $t("botSettings.hfModelMissing") }}
                <el-button
                    type="primary"
                    text
                    @click="downloadModels(settings.chatProvider.provider.model)"
                >
                    {{ $t("botSettings.hfModelDownloadLink") }}
                </el-button>
                {{ $t("botSettings.hfModelManual", { repo: chatModelRepository }) }}
            </template>
        </el-alert>
        <div v-if="showHfChatModelDownloadProgress" class="download-progress">
            <div class="download-progress-url">{{ $t("botSettings.downloading") }}: {{ downloadingUrl }}</div>
            <el-progress
                :percentage="Number(downloadingProgress) || 0"
                :stroke-width="14"
                striped
                striped-flow
            />
        </div>
    </el-card>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">
                {{ $t("botSettings.sentenceEmbedding") }}
                <el-tooltip effect="light" placement="right">
                    <template #content>
                        <span
                            v-html="$t('botSettings.sentenceEmbeddingTip')"
                        ></span>
                    </template>
                    <el-button circle>?</el-button>
                </el-tooltip>
            </div>
        </template>
        <el-row>
            <el-col :span="16">
                <el-form
                    :model="settings.sentenceEmbeddingProvider"
                    :label-width="formLabelWidth"
                >
                    <el-form-item :label="t('botSettings.provider')">
                        <el-radio-group
                            v-model="
                                settings.sentenceEmbeddingProvider.provider.id
                            "
                            @change="changeSentenceEmbeddingProvider"
                        >
                            <el-radio-button
                                v-for="item in sentenceEmbeddingProviders"
                                :id="item.id"
                                :key="item.id"
                                :label="item.id"
                                :value="item.id"
                            >
                                {{ $t(item.nameKey) }}
                            </el-radio-button>
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item
                        v-if="
                            settings.sentenceEmbeddingProvider.provider.id ==
                            'OpenAICompatible'
                        "
                        :label="t('botSettings.vendor')"
                    >
                        <el-select
                            v-model="sentenceEmbeddingVendorKey"
                            @change="applySentenceEmbeddingVendorPreset"
                        >
                            <el-option
                                v-for="item in vendorsFor('embedding')"
                                :key="item.key"
                                :label="$t(item.nameKey)"
                                :value="item.key"
                            />
                        </el-select>
                        <div class="form-item-help">
                            {{ $t("botSettings.embeddingApiHint") }}
                        </div>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="settings.sentenceEmbeddingProvider.apiUrl"
                            :disabled="
                                settings.sentenceEmbeddingProvider.apiUrlDisabled
                            "
                            @change="refreshSentenceEmbeddingVendor"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="$t('botSettings.apiKey')"
                        v-show="
                            settings.sentenceEmbeddingProvider.showApiKeyInput
                        "
                    >
                        <el-input
                            v-model="settings.sentenceEmbeddingProvider.apiKey"
                            show-password
                        />
                    </el-form-item>
                    <el-form-item :label="t('botSettings.model')">
                        <el-select
                            ref="sentenceEmbeddingModelSelector"
                            v-model="
                                settings.sentenceEmbeddingProvider.provider.model
                            "
                            :placeholder="$t('botSettings.chooseModel')"
                            :filterable="
                                settings.sentenceEmbeddingProvider.provider
                                    .id != 'HuggingFace'
                            "
                            :allow-create="
                                settings.sentenceEmbeddingProvider.provider
                                    .id != 'HuggingFace'
                            "
                        >
                            <el-option
                                v-for="item in sentenceEmbeddingModelOptions"
                                :id="item.value"
                                :key="item.value"
                                :label="item.label"
                                :value="item.value"
                            />
                            <template #footer>
                                <el-button
                                    :disabled="
                                        settings.sentenceEmbeddingProvider
                                            .provider.id == 'HuggingFace'
                                    "
                                    v-if="
                                        !isAddingAnotherSentenceEmbeddingModel
                                    "
                                    text
                                    bg
                                    @click="
                                        isAddingAnotherSentenceEmbeddingModel = true
                                    "
                                >
                                    {{ $t("botSettings.anotherModel") }}
                                </el-button>
                                <!-- 同 chat：只禁用按钮不够，表单本身也要挡住。 -->
                                <template
                                    v-else-if="
                                        settings.sentenceEmbeddingProvider
                                            .provider.id != 'HuggingFace'
                                    "
                                >
                                    <el-input
                                        v-model="anotherSentenceEmbeddingModel"
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherSentenceEmbeddingModel(
                                                anotherSentenceEmbeddingModel,
                                            )
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="
                                            isAddingAnotherSentenceEmbeddingModel = false
                                        "
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
                        <el-button
                            v-if="
                                settings.sentenceEmbeddingProvider.provider
                                    .id == 'OpenAICompatible'
                            "
                            :loading="sentenceEmbeddingModelListLoading"
                            :disabled="
                                !settings.sentenceEmbeddingProvider.apiUrl
                            "
                            style="margin-left: 8px"
                            @click="fetchSentenceEmbeddingModelList"
                        >
                            {{ $t("botSettings.fetchModelList") }}
                        </el-button>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.simThres')">
                        <div class="threshold-row">
                            ≥
                            <el-input-number
                                v-model="similarityThreshold"
                                :min="1"
                                :max="99"
                                :step="1"
                            />
                            %
                            <el-tooltip effect="light" placement="right">
                                <template #content>
                                    {{ $t("botSettings.simThresTip") }}
                                </template>
                                <el-button circle>?</el-button>
                            </el-tooltip>
                        </div>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.connTimeout')"
                        v-show="
                            settings.sentenceEmbeddingProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="
                                settings.sentenceEmbeddingProvider
                                    .connectTimeoutMillis
                            "
                            :min="100"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.readTimeout')"
                        v-show="
                            settings.sentenceEmbeddingProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="
                                settings.sentenceEmbeddingProvider
                                    .readTimeoutMillis
                            "
                            :min="500"
                            :max="65500"
                            :step="100"
                        />
                        <span class="form-item-suffix">{{
                            t("common.millis")
                        }}</span>
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.proxy')"
                        v-show="
                            settings.sentenceEmbeddingProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <div class="proxy-row">
                            <el-switch
                                v-model="sentenceEmbeddingProviderProxyEnabled"
                                :active-text="$t('common.enable')"
                            />
                            <el-input
                                v-model="settings.sentenceEmbeddingProvider.proxyUrl"
                                placeholder="http://127.0.0.1:9270"
                                :disabled="!sentenceEmbeddingProviderProxyEnabled"
                            />
                        </div>
                    </el-form-item>
                </el-form>
            </el-col>
            <el-col :span="7" :offset="1">
                <div class="usage-note">
                    {{ $t("botSettings.sentenceEmbeddingUsage") }}
                </div>
                <el-image
                    :src="sentenceEmbeddingPicThumbnail"
                    :zoom-rate="1.2"
                    :max-scale="7"
                    :min-scale="0.2"
                    :preview-src-list="usedBySentenceEmbeddingBig"
                    :initial-index="4"
                    fit="cover"
                />
            </el-col>
        </el-row>
        <el-alert
            v-if="showHfIncorrectEmbeddingModelTip"
            type="warning"
            :closable="false"
            class="hf-alert"
        >
            <template #title>
                {{ $t("botSettings.hfModelMissing") }}
                <el-button
                    type="primary"
                    text
                    @click="
                        downloadModels(
                            settings.sentenceEmbeddingProvider.provider.model,
                        )
                    "
                >
                    {{ $t("botSettings.hfModelDownloadLink") }}
                </el-button>
                {{ $t("botSettings.hfModelManual", { repo: sentenceEmbeddingModelRepository }) }}
            </template>
        </el-alert>
        <div
            v-if="showHfEmbeddingModelDownloadProgress"
            class="download-progress"
        >
            <div class="download-progress-url">{{ $t("botSettings.downloading") }}: {{ downloadingUrl }}</div>
            <el-progress
                :percentage="Number(downloadingProgress) || 0"
                :stroke-width="14"
                striped
                striped-flow
            />
        </div>
    </el-card>

    <el-card class="settings-card" shadow="never">
        <template #header>
            <div class="section-title">
                {{ $t("botSettings.smtp.title") }}
            </div>
        </template>
        <el-form :model="settings" :label-width="formLabelWidth">
            <el-form-item :label="$t('botSettings.smtp.host')">
                <el-input v-model="settings.smtpHost" />
            </el-form-item>
            <el-form-item :label="$t('botSettings.smtp.username')">
                <el-input v-model="settings.smtpUsername" />
            </el-form-item>
            <el-form-item :label="$t('botSettings.smtp.password')">
                <el-input
                    v-model="settings.smtpPassword"
                    type="password"
                    show-password
                />
            </el-form-item>
            <el-form-item :label="t('botSettings.connTimeout')">
                <el-input-number
                    v-model="settings.smtpTimeoutSec"
                    :min="1"
                    :max="600"
                />
                <span class="form-item-suffix">{{ t("common.sec") }}</span>
            </el-form-item>
            <el-form-item
                :label="$t('botSettings.smtp.emailRegex')"
                label-width="200px"
            >
                <el-input
                    v-model="settings.emailVerificationRegex"
                    :placeholder="defaultEmailVerificationRegex"
                />
                <div class="form-item-help">
                    {{ $t("botSettings.smtp.emailRegexHelp") }}
                </div>
            </el-form-item>
            <el-form-item label="">
                <el-button :loading="loading" type="info" @click="smtpTest">
                    {{ $t("botSettings.smtp.test") }}
                </el-button>
                <el-alert
                    v-if="smtpPassed"
                    :title="$t('botSettings.smtp.testPassed')"
                    type="success"
                    class="smtp-alert"
                />
                <el-alert
                    v-if="smtpFailed"
                    :title="smtpFailedDetail"
                    type="error"
                    class="smtp-alert"
                />
            </el-form-item>
        </el-form>
    </el-card>

    <div class="settings-footer">
        <el-button type="primary" @click="save">
            {{ $t("common.save") }}
        </el-button>
        <el-button @click="goBack()">{{ $t("common.back") }}</el-button>
    </div>
</template>
<style scoped>
.settings-card {
    border-radius: 12px;
    margin-bottom: 20px;
}

.settings-card :deep(.el-card__header) {
    padding: 14px 20px 0;
    border-bottom: none;
}

.settings-card .section-title {
    margin: 0;
}

.usage-note {
    color: var(--el-text-color-secondary, #909399);
    margin-bottom: 8px;
    font-size: 13px;
}

.form-item-suffix {
    margin-left: 8px;
    color: var(--el-text-color-secondary, #909399);
}

.form-item-help {
    width: 100%;
    color: var(--el-text-color-secondary, #909399);
    font-size: 12px;
    line-height: 1.5;
    margin-top: 4px;
}

.proxy-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
}

/* 开关不参与收缩，"启用" 等文案保持单行显示 */
.proxy-row :deep(.el-switch) {
    flex-shrink: 0;
}

.proxy-row :deep(.el-switch__label),
.proxy-row :deep(.el-switch__label *) {
    white-space: nowrap;
}

.threshold-row {
    display: flex;
    align-items: center;
    gap: 6px;
}

.hf-alert {
    margin-top: 8px;
}

.hf-alert :deep(.el-button) {
    padding: 0;
}

.download-progress {
    margin-top: 12px;
}

.download-progress-url {
    font-size: 12px;
    color: var(--el-text-color-secondary, #909399);
    margin-bottom: 4px;
    word-break: break-all;
}

.smtp-alert {
    margin-left: 12px;
    flex: 1;
}

.settings-footer {
    position: sticky;
    bottom: 0;
    z-index: 10;
    display: flex;
    gap: 4px;
    padding: 12px 20px;
    margin: 0 -20px;
    background: var(--el-bg-color, #fff);
    border-top: 1px solid var(--el-border-color-light, #e4e7ed);
}
</style>
