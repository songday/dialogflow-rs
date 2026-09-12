<script setup>
import { ref, reactive, onMounted, onUnmounted, provide } from "vue";
import { useRoute, useRouter } from "vue-router";
import { copyProperties, httpReq, getRobotType } from "../../assets/tools.js";
import { useI18n } from "vue-i18n";
import chatPicThumbnail from "@/assets/usedByLlmChatNode-thumbnail.png";
import chatPic from "@/assets/usedByLlmChatNode.png";
import textGenerationPicThumbnail from "@/assets/usedByDialogNodeTextGeneration-thumbnail.png";
import textGenerationPic from "@/assets/usedByDialogNodeTextGeneration.png";
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
    textGenerationProvider: {
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
const showHfIncorrectGenerationModelTip = ref(false);
const showHfGenerationModelDownloadProgress = ref(false);
const textGenerationModelRepository = ref("");
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
        await changeChatProvider(settings.chatProvider.provider.id);
        await changeTextGenerationProvider(
            settings.textGenerationProvider.provider.id,
        );
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
    if (settings.textGenerationProvider.provider.id == "HuggingFace") {
        for (let i = 0; i < textGenerationModelOptions.length; i++) {
            if (
                textGenerationModelOptions[i].value ==
                settings.textGenerationProvider.provider.model
            ) {
                let l = textGenerationModelOptions[i].value;
                const p = l.lastIndexOf(" ");
                if (p > -1) l = l.substring(0, p);
                textGenerationModelRepository.value = l;
                repostories.set(showHfIncorrectGenerationModelTip, l);
                break;
            }
        }
    } else showHfIncorrectGenerationModelTip.value = false;
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
            showHfIncorrectGenerationModelTip.value = false;
            showHfGenerationModelDownloadProgress.value = true;
        }
        timeoutID = setTimeout(async () => {
            await showDownloadProgress();
        }, 1000);
    });
}

function downloadComplete() {
    clearTimeout(timeoutID);
    showHfIncorrectGenerationModelTip.value = false;
    showHfGenerationModelDownloadProgress.value = false;
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
            showHfGenerationModelDownloadProgress.value = false;
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

// https://docs.spring.io/spring-ai/reference/api/chat/completions.html
const chatProviders = [
    {
        id: "HuggingFace",
        name: "HuggingFace",
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
        id: "OpenAI",
        name: "OpenAI",
        apiUrl: "https://api.openai.com/v1/chat/completions",
        apiUrlDisabled: true,
        showApiKeyInput: true,
        models: [
            { label: "gpt-4o", value: "gpt-4o" },
            { label: "gpt-4o-mini", value: "gpt-4o-mini" },
            { label: "gpt-4", value: "gpt-4" },
            { label: "gpt-4-turbo", value: "gpt-4-turbo" },
            { label: "gpt-4-vision-preview", value: "gpt-4-vision-preview" },
            { label: "gpt-4-32k", value: "gpt-4-32k" },
            { label: "gpt-3.5-turbo", value: "gpt-3.5-turbo" },
            { label: "gpt-3.5-turbo-16k", value: "gpt-3.5-turbo-16k" },
        ],
    },
    {
        id: "Ollama",
        name: "Ollama",
        apiUrl: "http://localhost:11434/api/chat",
        apiUrlDisabled: false,
        showApiKeyInput: false,
        models: ollamaModels,
    },
];

const chatProviderProxyEnabled = ref(false);
const isAddingAnotherChatOllamaModel = ref(false);
const anotherChatOllamaModel = ref("");
const chatModelSelector = ref();
const addAnotherChatOllamaModel = (m) => {
    const obj = { label: m, value: m };
    chatModelOptions.unshift(obj);
    chatProviders[2].models.unshift(obj);
    chatModelSelector.value.blur();
    settings.chatProvider.provider.id = "Ollama";
    settings.chatProvider.provider.model = obj.value;
    anotherChatOllamaModel.value = "";
};

const textGenerationProviders = [
    {
        id: "HuggingFace",
        name: "HuggingFace",
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
        id: "OpenAI",
        name: "OpenAI",
        apiUrl: "https://api.openai.com/v1/chat/completions",
        apiUrlDisabled: true,
        showApiKeyInput: true,
        models: [
            { label: "gpt-4", value: "gpt-4" },
            { label: "gpt-4-turbo-preview", value: "gpt-4-turbo-preview" },
            { label: "gpt-4-vision-preview", value: "gpt-4-vision-preview" },
            { label: "gpt-4-32k", value: "gpt-4-32k" },
            { label: "gpt-3.5-turbo", value: "gpt-3.5-turbo" },
            { label: "gpt-3.5-turbo-16k", value: "gpt-3.5-turbo-16k" },
        ],
    },
    {
        id: "Ollama",
        name: "Ollama",
        apiUrl: "http://localhost:11434/api/generate",
        apiUrlDisabled: false,
        showApiKeyInput: false,
        models: ollamaModels,
    },
];

const textGenerationProviderProxyEnabled = ref(false);
const isAddingAnotherTextGenerationOllamaModel = ref(false);
const anotherTextGenerationOllamaModel = ref("");
const textGenerationModelSelector = ref();
const addAnotherTextGenerationOllamaModel = (m) => {
    const obj = { label: m, value: m };
    textGenerationModelOptions.unshift(obj);
    textGenerationProviders[2].models.unshift(obj);
    textGenerationModelSelector.value.blur();
    settings.textGenerationProvider.provider.id = "Ollama";
    settings.textGenerationProvider.provider.model = obj.value;
    anotherTextGenerationOllamaModel.value = "";
};

// https://docs.spring.io/spring-ai/reference/api/embeddings.html
const sentenceEmbeddingProviders = [
    {
        id: "HuggingFace",
        name: "HuggingFace",
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
        id: "OpenAI",
        name: "OpenAI",
        apiUrl: "https://api.openai.com/v1/embeddings",
        apiUrlDisabled: true,
        showApiKeyInput: true,
        models: [
            {
                label: "text-embedding-3-large",
                value: "text-embedding-3-large",
            },
            {
                label: "text-embedding-3-small",
                value: "text-embedding-3-small",
            },
            {
                label: "text-embedding-ada-002",
                value: "text-embedding-ada-002",
            },
        ],
    },
    {
        id: "Ollama",
        name: "Ollama",
        apiUrl: "http://localhost:11434/api/embeddings",
        apiUrlDisabled: false,
        showApiKeyInput: false,
        models: [
            { label: "nomic-embed-text:v1.5", value: "nomic-embed-text:v1.5" },
            {
                label: "mxbai-embed-large:335m",
                value: "mxbai-embed-large:335m",
            },
            { label: "bge-m3:567m", value: "bge-m3:567m" },
            { label: "bge-large:335m", value: "bge-large:335m" },
            {
                label: "snowflake-arctic-embed:335m",
                value: "snowflake-arctic-embed:335m",
            },
            {
                label: "snowflake-arctic-embed2:568m",
                value: "snowflake-arctic-embed2:568m",
            },
            { label: "all-minilm:33m", value: "all-minilm:33m" },
            {
                label: "paraphrase-multilingual:278m",
                value: "paraphrase-multilingual:278m",
            },
            {
                label: "granite-embedding:278m",
                value: "granite-embedding:278m",
            },
            {
                label: "jina-embeddings-v2-base-en",
                value: "jina/jina-embeddings-v2-base-en:latest",
            },
        ],
    },
];
const sentenceEmbeddingProviderProxyEnabled = ref(false);
const chatModelOptions = reactive([]);
const chatDynamicReqUrlMap = new Map();
const choosedChatProvider = ref("");
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
                settings.chatProvider.apiUrl = chatDynamicReqUrlMap.get(
                    settings.chatProvider.provider.id,
                );
                if (!settings.chatProvider.apiUrl)
                    settings.chatProvider.apiUrl = chatProviders[i].apiUrl;
            }
            settings.chatProvider.apiUrlDisabled =
                chatProviders[i].apiUrlDisabled;
            settings.chatProvider.showApiKeyInput =
                chatProviders[i].showApiKeyInput;
            choosedChatProvider.value = n;
            if (n == "Ollama") {
                const r = await httpReq(
                    "GET",
                    "management/settings/model/ollama/list",
                    { url: chatProviders[i].apiUrl },
                    null,
                    null,
                );
                chatProviders[i].models.splice(
                    0,
                    chatProviders[i].models.length,
                    ...r.data.map((n) => ({ label: n, value: n })),
                );
                if (
                    chatProviders[i].models.find(
                        (d) => d.value == settings.chatProvider.provider.model,
                    ) == null
                ) {
                    addAnotherChatOllamaModel(
                        settings.chatProvider.provider.model,
                    );
                }
            }
            chatModelOptions.splice(
                0,
                chatModelOptions.length,
                ...chatProviders[i].models,
            );
            break;
        }
    }
};
const textGenerationModelOptions = reactive([]);
const textGenerationDynamicReqUrlMap = new Map();
const choosedTextGenerationProvider = ref("");
const changeTextGenerationProvider = async (n) => {
    if (choosedTextGenerationProvider.value)
        textGenerationDynamicReqUrlMap.set(
            choosedTextGenerationProvider.value,
            settings.textGenerationProvider.apiUrl,
        );
    for (let i = 0; i < textGenerationProviders.length; i++) {
        if (textGenerationProviders[i].id == n) {
            if (textGenerationProviders[i].apiUrlDisabled)
                settings.textGenerationProvider.apiUrl =
                    textGenerationProviders[i].apiUrl;
            else {
                settings.textGenerationProvider.apiUrl =
                    textGenerationDynamicReqUrlMap.get(
                        settings.textGenerationProvider.provider.id,
                    );
                if (!settings.textGenerationProvider.apiUrl)
                    settings.textGenerationProvider.apiUrl =
                        textGenerationProviders[i].apiUrl;
            }
            settings.textGenerationProvider.apiUrlDisabled =
                textGenerationProviders[i].apiUrlDisabled;
            settings.textGenerationProvider.showApiKeyInput =
                textGenerationProviders[i].showApiKeyInput;
            choosedTextGenerationProvider.value = n;
            if (n == "Ollama") {
                const r = await httpReq(
                    "GET",
                    "management/settings/model/ollama/list",
                    { url: textGenerationProviders[i].apiUrl },
                    null,
                    null,
                );
                textGenerationProviders[i].models.splice(
                    0,
                    textGenerationProviders[i].models.length,
                    ...r.data.map((n) => ({ label: n, value: n })),
                );
                if (
                    textGenerationProviders[i].models.find(
                        (d) =>
                            d.value ==
                            settings.textGenerationProvider.provider.model,
                    ) == null
                ) {
                    addAnotherTextGenerationOllamaModel(
                        settings.textGenerationProvider.provider.model,
                    );
                }
            }
            textGenerationModelOptions.splice(
                0,
                textGenerationModelOptions.length,
                ...textGenerationProviders[i].models,
            );
            break;
        }
    }
};
const sentenceEmbeddingModelOptions = reactive([]);
const sentenceEmbeddingDynamicReqUrlMap = new Map();
const choosedSentenceEmbeddingProvider = ref("");
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
                settings.sentenceEmbeddingProvider.apiUrl =
                    sentenceEmbeddingDynamicReqUrlMap.get(
                        settings.sentenceEmbeddingProvider.provider.id,
                    );
                if (!settings.sentenceEmbeddingProvider.apiUrl)
                    settings.sentenceEmbeddingProvider.apiUrl =
                        sentenceEmbeddingProviders[i].apiUrl;
            }
            settings.sentenceEmbeddingProvider.apiUrlDisabled =
                sentenceEmbeddingProviders[i].apiUrlDisabled;
            settings.sentenceEmbeddingProvider.showApiKeyInput =
                sentenceEmbeddingProviders[i].showApiKeyInput;
            choosedSentenceEmbeddingProvider.value = n;
            if (n == "Ollama") {
                const r = await httpReq(
                    "GET",
                    "management/settings/model/ollama/list",
                    { url: sentenceEmbeddingProviders[i].apiUrl },
                    null,
                    null,
                );
                sentenceEmbeddingProviders[i].models.splice(
                    0,
                    sentenceEmbeddingProviders[i].models.length,
                    ...r.data.map((n) => ({ label: n, value: n })),
                );
                if (
                    sentenceEmbeddingProviders[i].models.find(
                        (d) =>
                            d.value ==
                            settings.sentenceEmbeddingProvider.provider.model,
                    ) == null
                ) {
                    addAnotherSentenceEmbeddingOllamaModel(
                        settings.sentenceEmbeddingProvider.provider.model,
                    );
                }
            }
            sentenceEmbeddingModelOptions.splice(
                0,
                sentenceEmbeddingModelOptions.length,
                ...sentenceEmbeddingProviders[i].models,
            );
            break;
        }
    }
};

const sentenceEmbeddingModelSelector = ref();
const isAddingAnotherSentenceEmbeddingOllamaModel = ref(false);
const anotherSentenceEmbeddingOllamaModel = ref("");
const addAnotherSentenceEmbeddingOllamaModel = (m) => {
    const obj = { label: m, value: m };
    sentenceEmbeddingModelOptions.unshift(obj);
    sentenceEmbeddingProviders[2].models.unshift(obj);
    sentenceEmbeddingModelSelector.value.blur();
    settings.sentenceEmbeddingProvider.provider.id = "Ollama";
    settings.sentenceEmbeddingProvider.provider.model = obj.value;
    anotherSentenceEmbeddingOllamaModel.value = "";
};

const usedByLlmChatNodeBig = [chatPic];
const usedByTextGenerationBig = [textGenerationPic];
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
                            />
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="settings.chatProvider.apiUrl"
                            :disabled="settings.chatProvider.apiUrlDisabled"
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
                                        settings.chatProvider.provider.id !=
                                        'Ollama'
                                    "
                                    v-if="!isAddingAnotherChatOllamaModel"
                                    text
                                    bg
                                    @click="
                                        isAddingAnotherChatOllamaModel = true
                                    "
                                >
                                    {{ $t("botSettings.anotherOllamaModel") }}
                                </el-button>
                                <template v-else>
                                    <el-input
                                        v-model="anotherChatOllamaModel"
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherChatOllamaModel(
                                                anotherChatOllamaModel,
                                            )
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="
                                            isAddingAnotherChatOllamaModel = false
                                        "
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
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
                {{ $t("botSettings.txtGen") }}
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
                    :model="settings.textGenerationProvider"
                    :label-width="formLabelWidth"
                >
                    <el-form-item :label="t('botSettings.provider')">
                        <el-radio-group
                            v-model="settings.textGenerationProvider.provider.id"
                            @change="changeTextGenerationProvider"
                        >
                            <el-radio-button
                                v-for="item in textGenerationProviders"
                                :id="item.id"
                                :key="item.id"
                                :label="item.id"
                                :value="item.id"
                            />
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="settings.textGenerationProvider.apiUrl"
                            :disabled="
                                settings.textGenerationProvider.apiUrlDisabled
                            "
                        />
                    </el-form-item>
                    <el-form-item
                        :label="$t('botSettings.apiKey')"
                        v-show="settings.textGenerationProvider.showApiKeyInput"
                    >
                        <el-input
                            v-model="settings.textGenerationProvider.apiKey"
                            show-password
                        />
                    </el-form-item>
                    <el-form-item :label="t('botSettings.model')">
                        <el-select
                            ref="textGenerationModelSelector"
                            v-model="
                                settings.textGenerationProvider.provider.model
                            "
                            :placeholder="$t('botSettings.chooseModel')"
                        >
                            <el-option
                                v-for="item in textGenerationModelOptions"
                                :id="item.value"
                                :key="item.value"
                                :label="item.label"
                                :value="item.value"
                            />
                            <template #footer>
                                <el-button
                                    :disabled="
                                        settings.textGenerationProvider
                                            .provider.id != 'Ollama'
                                    "
                                    v-if="
                                        !isAddingAnotherTextGenerationOllamaModel
                                    "
                                    text
                                    bg
                                    @click="
                                        isAddingAnotherTextGenerationOllamaModel = true
                                    "
                                >
                                    {{ $t("botSettings.anotherOllamaModel") }}
                                </el-button>
                                <template v-else>
                                    <el-input
                                        v-model="
                                            anotherTextGenerationOllamaModel
                                        "
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherTextGenerationOllamaModel(
                                                anotherTextGenerationOllamaModel,
                                            )
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="
                                            isAddingAnotherTextGenerationOllamaModel = false
                                        "
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.maxResTokenLen')">
                        <el-input-number
                            v-model="
                                settings.textGenerationProvider
                                    .maxResponseTokenLength
                            "
                            :min="10"
                            :max="100000"
                            :step="5"
                        />
                    </el-form-item>
                    <el-form-item
                        :label="t('botSettings.connTimeout')"
                        v-show="
                            settings.textGenerationProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="
                                settings.textGenerationProvider
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
                            settings.textGenerationProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <el-input-number
                            v-model="
                                settings.textGenerationProvider
                                    .readTimeoutMillis
                            "
                            :min="1000"
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
                            settings.textGenerationProvider.provider.id !=
                            'HuggingFace'
                        "
                    >
                        <div class="proxy-row">
                            <el-switch
                                v-model="textGenerationProviderProxyEnabled"
                                :active-text="$t('common.enable')"
                            />
                            <el-input
                                v-model="settings.textGenerationProvider.proxyUrl"
                                placeholder="http://127.0.0.1:9270"
                                :disabled="!textGenerationProviderProxyEnabled"
                            />
                        </div>
                    </el-form-item>
                </el-form>
            </el-col>
            <el-col :span="7" :offset="1">
                <div class="usage-note">
                    {{ $t("botSettings.txtGenUsage") }}
                </div>
                <el-image
                    :src="textGenerationPicThumbnail"
                    :zoom-rate="1.2"
                    :max-scale="7"
                    :min-scale="0.2"
                    :preview-src-list="usedByTextGenerationBig"
                    :initial-index="4"
                    fit="cover"
                />
            </el-col>
        </el-row>
        <el-alert
            v-if="showHfIncorrectGenerationModelTip"
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
                            settings.textGenerationProvider.provider.model,
                        )
                    "
                >
                    {{ $t("botSettings.hfModelDownloadLink") }}
                </el-button>
                {{ $t("botSettings.hfModelManual", { repo: textGenerationModelRepository }) }}
            </template>
        </el-alert>
        <div
            v-if="showHfGenerationModelDownloadProgress"
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
                            />
                        </el-radio-group>
                    </el-form-item>
                    <el-form-item :label="t('botSettings.reqAddr')">
                        <el-input
                            v-model="settings.sentenceEmbeddingProvider.apiUrl"
                            :disabled="
                                settings.sentenceEmbeddingProvider.apiUrlDisabled
                            "
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
                                            .provider.id != 'Ollama'
                                    "
                                    v-if="
                                        !isAddingAnotherSentenceEmbeddingOllamaModel
                                    "
                                    text
                                    bg
                                    @click="
                                        isAddingAnotherSentenceEmbeddingOllamaModel = true
                                    "
                                >
                                    {{ $t("botSettings.anotherOllamaModel") }}
                                </el-button>
                                <template v-else>
                                    <el-input
                                        v-model="
                                            anotherSentenceEmbeddingOllamaModel
                                        "
                                        :placeholder="$t('botSettings.inputModelName')"
                                        style="margin-bottom: 8px"
                                    />
                                    <el-button
                                        type="primary"
                                        @click="
                                            addAnotherSentenceEmbeddingOllamaModel(
                                                anotherSentenceEmbeddingOllamaModel,
                                            )
                                        "
                                    >
                                        {{ $t("botSettings.confirm") }}
                                    </el-button>
                                    <el-button
                                        @click="
                                            isAddingAnotherSentenceEmbeddingOllamaModel = false
                                        "
                                        >{{ $t("botSettings.cancelLower") }}</el-button
                                    >
                                </template>
                            </template>
                        </el-select>
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
