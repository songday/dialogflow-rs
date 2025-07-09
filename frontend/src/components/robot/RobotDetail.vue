<script setup>
import { ref, reactive, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router';
import { copyProperties, httpReq, persistRobotDetail } from '../../assets/tools.js'
import Demos from "../Demos.vue"
import EpArrowRightBold from '~icons/ep/arrow-right-bold'
import BiChatSquareDots from '~icons/bi/chat-square-dots'
import MaterialSymbolsBook5Outline from '~icons/material-symbols/book-5-outline'
import RiBardLine from '~icons/ri/bard-line'
import SolarDownloadOutline from '~icons/solar/download-outline'
import SolarRouting2Linear from '~icons/solar/routing-2-linear'
import EpSetting from '~icons/ep/setting'
import SolarDocumentTextLinear from '~icons/solar/document-text-linear'
import BiBoxArrowUpRight from '~icons/bi/box-arrow-up-right'
const { t, locale } = useI18n();
const route = useRoute();
const router = useRouter();
// const fromPage = 'guide';
const robotId = route.params.robotId;
let robotNameForRestore = '';
const robotData = reactive({
  robotId: '',
  robotName: '',
  robotType: '',
})
const dialogFormVisible = ref(false)
const goBack = () => {
  router.push('/')
}
onMounted(async () => {
  const t = await httpReq('GET', 'robot/detail', { robotId: robotId }, null, null);
  if (t.status == 200 && t.data != null) {
    copyProperties(t.data, robotData)
    robotNameForRestore = robotData.robotName;
    persistRobotDetail(t.data);
    // persistRobotType(t.data.robotId, t.data.robotType);
  } else {
    ElMessage.error('Can NOT find robot information by robotId.');
  }
})
async function updateRobot() {
  const t = await httpReq('POST', 'robot', null, null, robotData);
  // console.log(t.data);
  if (t.status == 200)
    ElMessage.success('Changed successfully.');
  else
    ElMessage.error(t.err.message);
}
async function deleteRobot() {
  ElMessageBox.confirm(
    t('guide.delRoConfirm'),
    'Warning',
    {
      confirmButtonText: 'OK',
      cancelButtonText: 'Cancel',
      type: 'warning',
    }
  )
    .then(async () => {
      const t = await httpReq('DELETE', 'robot', { robotId: robotId }, null, null);
      // console.log(t.data);
      if (t.status == 200)
        goBack();
      else
        ElMessage.error(t.err.message);
    })
    .catch(() => {
      // ElMessage({
      //     type: 'info',
      //     message: 'Delete canceled',
      // })
    })
}
</script>
<style scoped>
.header-row {
  margin-top: 20px;
}

.header {
  font-size: 38px;
  font-weight: bold;
}

.title {
  font-size: 28px;
  font-weight: bold;
  margin-top: 35px;
}

.description {
  font-size: 16px;
  color: #b8b8b8;
  padding-bottom: 20px;
  border-bottom: #b8b8b8 1px solid;
}

.tips {
  text-align: right;
  margin-right: 30px;
}
</style>
<template>
  <el-page-header :title="t('guide.header1')" @back="goBack">
    <template #content>
      <span class="text-large font-600 mr-3">{{ t('guide.header2') }}</span>
    </template>
  </el-page-header>
  <el-row class="header-row">
    <el-col :span="24">
      <span class="header"> {{ robotData.robotName }}</span>
      <el-button type="primary" text @click="dialogFormVisible = true;">
        {{ t('guide.chRoNaBtn') }}
      </el-button>
    </el-col>
  </el-row>
  <el-row>
    <el-col :span="20">
      {{ t('guide.robotId') }}: {{ robotId }}
    </el-col>
    <el-col :span="2">
      <el-button type="danger" @click="deleteRobot">
        {{ t('guide.delRoNaBtn') }}
      </el-button>
    </el-col>
  </el-row>
  <div style="margin-left:50px">
    <div class="title">
      <el-icon :size="30">
        <BiChatSquareDots />
      </el-icon>{{ $t('guide.title1') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'mainflows', params: { robotId: robotId } }">{{ $t('guide.nav1') }}</router-link>
      <div class="description">
        <Demos :parentPage="robotDetail" />
        <!-- <router-link :to="{ name: 'subflow', params: { id: 'demo-repay', name: btoa('Repay Demo') } }">
            {{ $t('home.demo1') }}
          </router-link>
          |
          <router-link
            :to="{ name: 'subflow', params: { id: 'demo-collect', name: btoa('Information Collection Demo') } }">
            {{ $t('home.demo2') }}
          </router-link>
          |
          <router-link
            :to="{ name: 'subflow', params: { id: 'demo-notify', name: btoa('One Sentence Notification Demo') } }">
            {{ $t('home.demo3') }}
          </router-link> -->
      </div>
    </div>

    <div class="title">
      <el-icon :size="30">
        <MaterialSymbolsBook5Outline />
      </el-icon>
      {{ t('menu.kb') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'kbQA', params: { robotId: robotId } }">{{ t('menu.qa') }}</router-link>
      &nbsp;&nbsp;
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'kbDoc', params: { robotId: robotId } }">{{ t('menu.doc') }}</router-link>
      <div class="description">
        {{ $t('guide.kbDesc') }}<br />
        {{ $t('guide.qaDesc') }}
      </div>
    </div>

    <div class="title">
      <el-icon :size="30">
        <RiBardLine />
      </el-icon>
      {{ $t('guide.title2') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'intents', params: { robotId: robotId } }">{{ $t('guide.nav2') }}</router-link>
      <div class="description">
        {{ $t('guide.desc2') }}<br />
        {{ $t('guide.intentsDesc') }}
      </div>
    </div>

    <div class="title">
      <el-icon :size="30">
        <SolarDownloadOutline />
      </el-icon>
      {{ $t('guide.title3') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'variables', params: { robotId: robotId } }">{{ $t('guide.nav3') }}</router-link>
      <div class="description">{{ $t('guide.desc3') }}</div>
    </div>

    <div class="title">
      <el-icon :size="30">
        <SolarRouting2Linear />
      </el-icon>
      {{ $t('guide.eApiTitle') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'externalHttpApis', params: { robotId: robotId } }">{{ $t('guide.httpApiList') }}</router-link>
      <div class="description">{{ $t('guide.eApiDesc') }}</div>
    </div>

    <div class="title">
      <el-icon :size="30">
        <EpSetting />
      </el-icon>
      {{ $t('guide.title4') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <router-link :to="{ name: 'settings', params: { robotId: robotId } }">{{ $t('guide.nav4') }}</router-link>
      <div class="description">{{ $t('guide.desc4') }}</div>
    </div>

    <div class="title">
      <el-icon :size="30">
        <SolarDocumentTextLinear />
      </el-icon>
      {{ $t('guide.title5') }}
    </div>
    <div>
      <el-icon :size="15">
        <EpArrowRightBold />
      </el-icon>
      <!-- <router-link to="/doc">{{ $t('guide.nav5') }}</router-link> -->
      <a href="https://dialogflowai.github.io/doc" target="_blank">
        {{ $t('guide.nav5') }}
        <el-icon>
          <BiBoxArrowUpRight />
        </el-icon>
      </a>
      <div class="description">{{ $t('guide.desc5') }}</div>
    </div>
  </div>
  <el-dialog v-model="dialogFormVisible" :title="t('guide.chRoNaBtn')">
    <el-form :model="form">
      <el-form-item :label="t('common.name')" :label-width="formLabelWidth">
        <el-input v-model="robotData.robotName" autocomplete="off" />
      </el-form-item>
    </el-form>
    <template #footer>
      <span class="dialog-footer">
        <el-button type="primary" @click="dialogFormVisible = false; updateRobot();">
          {{ $t('common.save') }}
        </el-button>
        <el-button @click="robotData.robotName = robotNameForRestore; dialogFormVisible = false">{{
          $t('common.cancel') }}</el-button>
      </span>
    </template>
  </el-dialog>
</template>