<script setup>
// Method: GET
// POST Content-Type: application/x-www-form-urlencoded
// Content-Length: 13
// enctype: application/x-www-form-urlencoded

// enctype to multipart/form-data
import { ref, reactive, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
// import { ElMessage } from 'element-plus'
import { cloneObj, copyProperties, httpReq } from '../../assets/tools.js'
import { useI18n } from 'vue-i18n'
import SolarRouting2Linear from '~icons/solar/routing-2-linear'
import EpPlus from '~icons/ep/plus'
const { t, tm, rt } = useI18n();
const route = useRoute();
const router = useRouter();
const robotId = route.params.robotId;
const httpApiData = reactive({
  id: '',
  name: '',
  description: '',
  protocol: 'http://',
  method: 'GET',
  address: '',
  // timeoutMilliseconds: '1500',
  postContentType: 'UrlEncoded',
  headers: [],
  queryParams: [],
  formData: [],
  requestBody: '',
  userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/123.0',
  // asyncReq: false,
})
const param = reactive({
  name: '',
  value: '',
  valueSource: '',
})
const setFormVisible = ref(false)
const varDialogVisible = ref(false)
const dynamicTitle = ref('')
const activeName = ref('h')
const editIdx = ref(0)
const vars = reactive([])
const selectedVar = ref('')
const requestBodyRef = ref()
const apiId = route.params.id;
onMounted(async () => {
  if (apiId && apiId != 'new') {
    const t = await httpReq('GET', 'external/http/' + apiId, { robotId: robotId }, null, null);
    // console.log(t);
    if (t && t.status == 200) {
      copyProperties(t.data, httpApiData);
    }
  }
  let t = await httpReq('GET', 'variable', { robotId: robotId }, null, null);
  if (t && t.status == 200 && t.data) {
    for (var x in t.data) {
      if (t.data.hasOwnProperty(x)) {
        vars.push(t.data[x]);
      }
    }
  }
})
const tabCounts = () => {
  if (activeName.value == 'h')
    return httpApiData.headers.length
  else if (activeName.value == 'q')
    return httpApiData.queryParams.length
  else if (activeName.value == 'f')
    return httpApiData.formData.length
}
const newParam = () => {
  param.name = '';
  param.value = '';
  param.valueSource = 'Val';
  editIdx.value = -1;
  const p = activeName.value;
  if (p == 'h')
    dynamicTitle.value = t('eApi.detail.addHeaderTitle')
  else if (p == 'q')
    dynamicTitle.value = t('eApi.detail.addQueryParamTitle')
  else if (p == 'f')
    dynamicTitle.value = t('eApi.detail.addFormTitle')
  setFormVisible.value = true;
}
const addParam = () => {
  const p = cloneObj(param);
  const idx = editIdx.value;
  if (idx > -1) {
    if (activeName.value == 'h')
      httpApiData.headers[idx] = p;
    else if (activeName.value == 'q')
      httpApiData.queryParams[idx] = p;
    else if (activeName.value == 'f')
      httpApiData.formData[idx] = p;
  } else {
    if (activeName.value == 'h')
      httpApiData.headers.push(p);
    else if (activeName.value == 'q')
      httpApiData.queryParams.push(p);
    else if (activeName.value == 'f')
      httpApiData.formData.push(p);
  }
  setFormVisible.value = false
}
const editParam = (idx) => {
  editIdx.value = idx;
  if (activeName.value == 'h')
    copyProperties(httpApiData.headers[idx], param)
  else if (activeName.value == 'q')
    copyProperties(httpApiData.queryParams[idx], param)
  else if (activeName.value == 'f')
    copyProperties(httpApiData.formData[idx], param)
  const p = activeName.value;
  if (p == 'h')
    dynamicTitle.value = t('eApi.detail.editHeaderTitle')
  else if (p == 'q')
    dynamicTitle.value = t('eApi.detail.editQueryParamTitle')
  else if (p == 'f')
    dynamicTitle.value = t('eApi.detail.editFormTitle')
  setFormVisible.value = true
}
const delParam = (idx) => {
  if (activeName.value == 'h')
    httpApiData.headers.splice(idx, 1)
  else if (activeName.value == 'q')
    httpApiData.queryParams.splice(idx, 1)
  else if (activeName.value == 'f')
    httpApiData.formData.splice(idx, 1)
}
const save = async () => {
  httpApiData.protocol = httpApiData.protocol.replace('://', '').toUpperCase();
  const resp = await httpReq('POST', 'external/http/' + apiId, { robotId: robotId }, null, httpApiData);
  // console.log(resp);
  if (resp && resp.status == 200) {
    ElMessage({
      showClose: true,
      message: t('eApi.detail.savedTip'),
      type: 'success',
    });
    goBack();
  } else {
    ElMessage({
      showClose: true,
      message: t('eApi.detail.errTip'),
      type: 'error',
    })
  }
}
const insertVar = () => {
  httpApiData.requestBody += '`' + selectedVar.value + '`'
  varDialogVisible.value = false
}
const goBack = () => {
  router.push({ name: 'externalHttpApis', params: { robotId: robotId } });
}
const changeTab = (v) => {
  if (v != 'POST' && activeName.value == 'f')
    activeName.value = 'q'
}
</script>
<style scoped>
.mainBody {
  max-width: 980px;
}

.url-row {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
}

.url-row .url-input {
  flex: 1;
}

.method-select {
  width: 118px;
  flex-shrink: 0;
}

.protocol-select {
  width: 106px;
  flex-shrink: 0;
}

/* Color the method by its verb */
.method-select :deep(.el-input__inner) {
  font-weight: 600;
}

.method-get :deep(.el-input__inner) {
  color: #16a34a;
}

.method-post :deep(.el-input__inner) {
  color: #d97706;
}

.param-table {
  margin-bottom: 14px;
}

.add-param-btn {
  margin-top: 2px;
}

.empty-hint {
  padding: 18px 0 6px;
  color: var(--app-text-secondary, #6b7280);
  font-size: 13px;
}

.body-type-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 14px;
  color: var(--app-text-secondary, #6b7280);
  font-size: 13px;
}

.value-row {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
}

.source-select {
  width: 160px;
  flex-shrink: 0;
}

.value-row .value-input {
  flex: 1;
}

.var-tag {
  margin-left: 6px;
}

.my-header {
  display: flex;
  flex-direction: row;
  justify-content: space-between;
}
</style>
<template>
  <div class="mainBody">
    <div class="page-header">
      <h1 class="page-title">
        <span class="page-title-icon"><SolarRouting2Linear /></span>
        {{ t('eApi.detail.title') }}
      </h1>
      <div class="page-actions">
        <el-button @click="goBack">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="save">{{ t('common.save') }}</el-button>
      </div>
    </div>

    <div class="page-card">
      <el-form :model="httpApiData" label-width="90px">
        <el-form-item :label="t('eApi.detail.apiName')">
          <el-input v-model="httpApiData.name" :placeholder="t('eApi.detail.apiNamePh')" />
        </el-form-item>
        <el-form-item :label="t('common.desc')">
          <el-input v-model="httpApiData.description" maxlength="256" :placeholder="t('eApi.detail.descPh')"
            show-word-limit type="textarea" :rows="2" />
        </el-form-item>
        <el-form-item :label="t('eApi.detail.requestUrl')">
          <div class="url-row">
            <el-select v-model="httpApiData.method" placeholder="" class="method-select"
              :class="'method-' + httpApiData.method.toLowerCase()" @change="changeTab">
              <el-option label="GET" value="GET" />
              <el-option label="POST" value="POST" />
            </el-select>
            <el-select v-model="httpApiData.protocol" placeholder="" class="protocol-select">
              <el-option label="HTTP" value="http://" />
              <el-option label="HTTPS" value="https://" />
            </el-select>
            <el-input v-model="httpApiData.address" class="url-input" placeholder="api.example.com/v1/endpoint" />
          </div>
        </el-form-item>
      </el-form>
    </div>

    <div class="section-title">{{ t('eApi.detail.advanced') }}</div>
    <div class="page-card">
      <el-form :model="httpApiData" label-width="90px">
        <el-form-item :label="t('eApi.detail.parameters')">
          <el-tabs v-model="activeName" class="demo-tabs" style="width: 100%;">
            <el-tab-pane :label="t('eApi.detail.header') + ' (' + httpApiData.headers.length + ')'" name="h">
              <el-table :data="httpApiData.headers" stripe class="param-table" style="width: 100%">
                <el-table-column prop="name" :label="t('eApi.detail.paramName')" min-width="240" />
                <el-table-column :label="t('eApi.detail.paramValue')" min-width="200">
                  <template #default="scope">
                    <span>{{ scope.row.value }}</span>
                    <el-tag v-if="scope.row.valueSource == 'Var'" size="small" type="info" class="var-tag">
                      {{ t('eApi.detail.varSource') }}
                    </el-tag>
                  </template>
                </el-table-column>
                <el-table-column fixed="right" :label="tm('mainflow.table')[2]" width="180" align="center">
                  <template #default="scope">
                    <el-button link type="primary" size="small" @click="editParam(scope.$index)">
                      {{ t('eApi.detail.edit') }}
                    </el-button>
                    <el-button link type="danger" size="small" @click="delParam(scope.$index)">
                      {{ t('eApi.detail.del') }}
                    </el-button>
                  </template>
                </el-table-column>
                <template #empty>
                  <div class="empty-hint">{{ t('eApi.detail.noHeaders') }}</div>
                </template>
              </el-table>
              <el-button type="primary" plain class="add-param-btn" @click="newParam">
                <el-icon style="margin-right: 6px"><EpPlus /></el-icon>{{ t('eApi.detail.addHeader') }}
              </el-button>
            </el-tab-pane>
            <el-tab-pane
              :label="t('eApi.detail.queryParams') + ' (' + httpApiData.queryParams.length + ')'" name="q">
              <el-table :data="httpApiData.queryParams" stripe class="param-table" style="width: 100%">
                <el-table-column prop="name" :label="t('eApi.detail.paramName')" min-width="240" />
                <el-table-column :label="t('eApi.detail.paramValue')" min-width="200">
                  <template #default="scope">
                    <span>{{ scope.row.value }}</span>
                    <el-tag v-if="scope.row.valueSource == 'Var'" size="small" type="info" class="var-tag">
                      {{ t('eApi.detail.varSource') }}
                    </el-tag>
                  </template>
                </el-table-column>
                <el-table-column fixed="right" :label="tm('mainflow.table')[2]" width="180" align="center">
                  <template #default="scope">
                    <el-button link type="primary" size="small" @click="editParam(scope.$index)">
                      {{ t('eApi.detail.edit') }}
                    </el-button>
                    <el-button link type="danger" size="small" @click="delParam(scope.$index)">
                      {{ t('eApi.detail.del') }}
                    </el-button>
                  </template>
                </el-table-column>
                <template #empty>
                  <div class="empty-hint">{{ t('eApi.detail.noQueryParams') }}</div>
                </template>
              </el-table>
              <el-button type="primary" plain class="add-param-btn" @click="newParam">
                <el-icon style="margin-right: 6px"><EpPlus /></el-icon>{{ t('eApi.detail.addQueryParam') }}
              </el-button>
            </el-tab-pane>
            <el-tab-pane :label="t('eApi.detail.requestBody')" name="f" v-if="httpApiData.method == 'POST'">
              <div class="body-type-row">
                {{ t('eApi.detail.bodyType') }}:
                <el-radio-group v-model="httpApiData.postContentType">
                  <el-radio-button value="UrlEncoded">x-www-form-urlencoded</el-radio-button>
                  <el-radio-button value="JSON">JSON</el-radio-button>
                </el-radio-group>
              </div>
              <el-table v-if="httpApiData.postContentType == 'UrlEncoded'" :data="httpApiData.formData" stripe
                class="param-table" style="width: 100%">
                <el-table-column prop="name" :label="t('eApi.detail.paramName')" min-width="240" />
                <el-table-column :label="t('eApi.detail.paramValue')" min-width="200">
                  <template #default="scope">
                    <span>{{ scope.row.value }}</span>
                    <el-tag v-if="scope.row.valueSource == 'Var'" size="small" type="info" class="var-tag">
                      {{ t('eApi.detail.varSource') }}
                    </el-tag>
                  </template>
                </el-table-column>
                <el-table-column fixed="right" :label="tm('mainflow.table')[2]" width="180" align="center">
                  <template #default="scope">
                    <el-button link type="primary" size="small" @click="editParam(scope.$index)">
                      {{ t('eApi.detail.edit') }}
                    </el-button>
                    <el-button link type="danger" size="small" @click="delParam(scope.$index)">
                      {{ t('eApi.detail.del') }}
                    </el-button>
                  </template>
                </el-table-column>
                <template #empty>
                  <div class="empty-hint">{{ t('eApi.detail.noFormData') }}</div>
                </template>
              </el-table>
              <el-button type="primary" plain class="add-param-btn" v-if="httpApiData.postContentType == 'UrlEncoded'"
                @click="newParam">
                <el-icon style="margin-right: 6px"><EpPlus /></el-icon>{{ t('eApi.detail.addFormData') }}
              </el-button>
              <template v-if="httpApiData.postContentType == 'JSON'">
                <el-input ref="requestBodyRef" v-model="httpApiData.requestBody" maxlength="10240" placeholder="JSON"
                  show-word-limit type="textarea" :rows="8" />
                <el-button type="primary" plain class="add-param-btn" @click="varDialogVisible = true">
                  <el-icon style="margin-right: 6px"><EpPlus /></el-icon>{{ t('eApi.detail.insertVar') }}
                </el-button>
              </template>
            </el-tab-pane>
          </el-tabs>
        </el-form-item>
        <el-form-item :label="t('eApi.detail.userAgent')">
          <el-input v-model="httpApiData.userAgent" />
        </el-form-item>
      </el-form>
    </div>

    <el-dialog v-model="setFormVisible" width="560px" destroy-on-close>
      <template #header="{ close, titleId, titleClass }">
        <div class="my-header">
          <h4 :id="titleId" :class="titleClass">{{ dynamicTitle }}</h4>
        </div>
      </template>
      <el-form :model="param" label-width="60px">
        <el-form-item :label="t('eApi.detail.pName')">
          <el-input v-model="param.name" autocomplete="off" :placeholder="t('eApi.detail.paramName')" />
        </el-form-item>
        <el-form-item :label="t('eApi.detail.pValue')">
          <div class="value-row">
            <el-select v-model="param.valueSource" placeholder="" class="source-select">
              <el-option :label="t('eApi.detail.constValue')" value="Val" />
              <el-option :label="t('eApi.detail.fromVar')" value="Var" />
            </el-select>
            <el-input v-if="param.valueSource == 'Val'" v-model="param.value" autocomplete="off"
              class="value-input" />
            <el-select v-if="param.valueSource == 'Var'" v-model="selectedVar"
              :placeholder="t('eApi.detail.selectVar')" class="value-input">
              <el-option v-for="item in vars" :key="item.varName" :label="item.varName" :value="item.varName" />
            </el-select>
          </div>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="setFormVisible = false">{{ $t('common.cancel') }}</el-button>
        <el-button type="primary" @click="addParam">{{ $t('common.save') }}</el-button>
      </template>
    </el-dialog>
    <el-dialog v-model="varDialogVisible" :title="t('eApi.detail.insertVar')" width="420px" :append-to-body="true"
      :destroy-on-close="true">
      <el-select v-model="selectedVar" :placeholder="t('eApi.detail.chooseVar')" size="large" style="width: 100%;">
        <el-option v-for="item in vars" :key="item.varName" :label="item.varName" :value="item.varName" />
      </el-select>
      <template #footer>
        <span class="dialog-footer">
          <el-button @click="varDialogVisible = false">{{ t('common.cancel') }}</el-button>
          <el-button type="primary" @click="insertVar">
            {{ t('common.insert') }}
          </el-button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>
