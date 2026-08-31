<script setup>
import { reactive, ref, onMounted, nextTick } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { httpReq } from '../../assets/tools.js'
import { useI18n } from 'vue-i18n'
const { t, tm, rt } = useI18n();
import MaterialSymbolsBook5Outline from '~icons/material-symbols/book-5-outline';
import EpPlus from '~icons/ep/plus';
const route = useRoute()
const router = useRouter();
const robotId = route.params.robotId
const qaData = reactive({
    id: null,
    question: {
        question: ''
    },
    similarQuestions: [],
    answer: '',
})
const tableData = reactive([])
const objectSpanMethod = ({ row, column, rowIndex, columnIndex }) => {
    console.log(column);
    return { rowspan: column.length, colspan: 1 }
}
const listQa = async () => {
    const t = await httpReq('GET', 'kb/qa', { robotId: robotId }, null, null);
    console.log(t);
    if (t.status == 200)
        tableData.splice(0, tableData.length, ...t.data);
}
onMounted(() => {
    listQa();
})
const newQa = () => {
    qaData.id = null;
    qaData.question.question = '';
    qaData.similarQuestions = [];
    qaData.answer = ''
    dialogVisible.value = true
}
const showQaDetail = (idx) => {
    qaDetailIdx.value = idx
    const d = tableData[idx];
    if (d) {
        qaData.id = d.id;
        qaData.question.question = d.question.question;
        qaData.similarQuestions.splice(0, qaData.similarQuestions.length, ...d.similarQuestions);
        qaData.answer = d.answer
        qaDetailVisible.value = true
    }
}
const editQa = (idx) => {
    const d = tableData[idx];
    if (d) {
        qaData.id = d.id;
        qaData.question.question = d.question.question;
        qaData.similarQuestions.splice(0, qaData.similarQuestions.length, ...d.similarQuestions);
        qaData.answer = d.answer
        dialogVisible.value = true
    }
}
const saveQa = async () => {
    const t = await httpReq('POST', 'kb/qa', { robotId: robotId }, null, qaData);
    console.log(t);
    dialogVisible.value = false
    listQa()
}
const deleteQa = async (idx) => {
    ElMessageBox.confirm(
        'Confirm to delete this QnA?',
        'Warning',
        {
            confirmButtonText: t('common.del'),
            cancelButtonText: t('common.cancel'),
            type: 'warning',
        }
    ).then(async () => {
        const d = tableData[idx];
        if (d) {
            qaData.id = d.id;
            const t = await httpReq('DELETE', 'kb/qa', { robotId: robotId }, null, qaData);
            console.log(t);
            nextTick(() => {
                qaDetailVisible.value = false
                listQa()
            })
        }
    }).catch(() => {
        // ElMessage({
        //     type: 'info',
        //     message: 'Delete canceled',
        // })
    })
}
const testQa = (text) => {
    loading.value = true;
    (async function (text) {
        const t = await httpReq('GET', 'kb/qa/dryrun', { robotId: robotId, text: text }, null, null);
        console.log(t);
        if (t.status == 200)
            testQnAResult.value = t.data[0].answer + ' (Distance: ' + t.data[1] + ')';
        else
            testQnAResult.value = t.err.message;
    })(text).then(() => loading.value = false);
}
const goBack = () => {
    router.push({ name: 'robotDetail', params: { robotId: robotId } });
}

const dialogVisible = ref(false)
const qaDetailVisible = ref(false)
const qaDetailIdx = ref(0)
const dryRunFormVisible = ref(false)
const loading = ref(false)
const testQnAText = ref('')
const testQnAResult = ref('')
const formLabelWidth = '120px'
</script>
<style scoped>
.similar-count {
    display: inline-block;
    min-width: 28px;
    padding: 2px 10px;
    border-radius: 999px;
    text-align: center;
    font-size: 13px;
    font-weight: 600;
    background: #eef2ff;
    color: #6366f1;
}

.qa-answer {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    color: #4e5969;
}
</style>
<template>
    <div class="page-header">
        <h1 class="page-title">
            <span class="page-title-icon"><MaterialSymbolsBook5Outline /></span>
            Questions and answer
        </h1>
        <div class="page-actions">
            <el-button @click="dryRunFormVisible = true">Test QnA</el-button>
            <el-button type="primary" @click="newQa">
                <el-icon style="margin-right: 6px"><EpPlus /></el-icon>
                Add QnA pair
            </el-button>
        </div>
    </div>
    <div class="page-card">
        <el-table :data="tableData" stripe style="width: 100%">
            <el-table-column prop="question.question" label="Question" min-width="300" />
            <el-table-column label="No. of similar questions" width="190" align="center">
                <template #default="scope">
                    <span class="similar-count">{{ scope.row.similarQuestions.length }}</span>
                </template>
            </el-table-column>
            <el-table-column prop="answer" label="Answer" min-width="240">
                <template #default="scope">
                    <span class="qa-answer">{{ scope.row.answer }}</span>
                </template>
            </el-table-column>
            <el-table-column fixed="right" label="Operations" width="200" align="center">
                <template #default="scope">
                    <el-button link type="primary" @click="showQaDetail(scope.$index)">Detail</el-button>
                    <el-button link type="primary" @click="editQa(scope.$index)">Edit</el-button>
                    <el-button link type="danger" @click="deleteQa(scope.$index)">Delete</el-button>
                </template>
            </el-table-column>
        </el-table>
    </div>
    <el-dialog v-model="dialogVisible" title="Add new QA" width="720px" destroy-on-close>
        <el-form :model="qaData">
            <el-form-item label="Question" :label-width="formLabelWidth">
                <el-input v-model="qaData.question.question" placeholder="The question users may ask" />
            </el-form-item>
            <el-form-item v-for="(item, index) in qaData.similarQuestions" :id="index" :key="index"
                :label="index == 0 ? 'Similar questions' : ''" :label-width="formLabelWidth">
                <el-input v-model="qaData.similarQuestions[index].question" placeholder="A variant phrasing of the question"
                    style="width: 90%;" />
                <el-button circle type="danger" plain @click="qaData.similarQuestions.splice(index, 1)">-</el-button>
            </el-form-item>
            <el-form-item label="" :label-width="formLabelWidth">
                <el-button plain @click="qaData.similarQuestions.push({ question: '' })">Add similar
                    question</el-button>
            </el-form-item>
            <el-form-item label="Answer" :label-width="formLabelWidth">
                <el-input v-model="qaData.answer" placeholder="The answer given to users" type="textarea" :rows="5" />
            </el-form-item>
        </el-form>
        <template #footer>
            <div class="dialog-footer">
                <el-button @click="dialogVisible = false">Cancel</el-button>
                <el-button type="primary" @click="saveQa">
                    {{ $t('common.save') }}
                </el-button>
            </div>
        </template>
    </el-dialog>
    <el-drawer v-model="qaDetailVisible" title="Detail of QnA" direction="rtl" size="480px">
        <el-form>
            <el-form-item label="Question" :label-width="formLabelWidth">
                {{ qaData.question.question }}
            </el-form-item>
            <el-form-item label="Similar questions" :label-width="formLabelWidth"
                v-show="qaData.similarQuestions.length > 0">
                <div v-for="(item, idx) in qaData.similarQuestions" :id="idx" :key="idx">
                    {{ item.question }}
                </div>
            </el-form-item>
            <el-form-item label="Answer" :label-width="formLabelWidth">
                {{ qaData.answer }}
            </el-form-item>
        </el-form>
        <div class="demo-drawer__footer">
            <el-button type="primary" @click="dialogVisible = true">Edit</el-button>
            <el-button type="danger" @click="deleteQa(qaDetailIdx)">Delete</el-button>
            <el-button @click="qaDetailVisible = false">Close</el-button>
        </div>
    </el-drawer>
    <el-drawer v-model="dryRunFormVisible" title="Test QnA" direction="rtl" size="480px">
        <el-form>
            <el-form-item label="">
                <el-input v-model="testQnAText" placeholder="Please input some texts" clearable
                    @keyup.enter="testQa(testQnAText)" />
            </el-form-item>
            <el-form-item label="">
                <el-alert v-if="testQnAResult" :title="testQnAResult" type="info" :closable="false" />
            </el-form-item>
        </el-form>
        <div class="demo-drawer__footer">
            <el-button type="primary" :loading="loading" @click="testQa(testQnAText)">Test</el-button>
            <el-button @click="dryRunFormVisible = false">Close</el-button>
        </div>
    </el-drawer>
</template>