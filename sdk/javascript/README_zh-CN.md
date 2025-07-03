## 直接引入SDK

> 假设本工具的地址是：http://127.0.0.1:12715

### script 标签
```html
<script src="http://127.0.0.1:12715/assets/DialogFlowAiSDK.min.js"></script>
```

### ES6 模块
```javascript
import { DialogFlowAiSDK } from 'http://127.0.0.1:12715/assets/DialogFlowAiSDK.min.js'
```

## 使用

> 我们以 Vue3 来做示例

```javascript
// 是否在等待答案
const waitingResponse = ref(false)
// 用户输入
const userAsk = ref('')
// 聊天记录
const chatRecords = ref([])
// SDK 变量
let dialogFlowAiSDK = null;
async function dryrun() {
    // 如果用户没有输入，就不请求应答接口
    if (chatRecords.value.length > 0 && !userAsk.value)
        return;
    // 检查是否在等待应答接口返回，避免重复应答
    if (waitingResponse.value)
        return;
    waitingResponse.value = true;
    if (dialogFlowAiSDK == null) {
        dialogFlowAiSDK = new DialogFlowAiSDK({
            url: 'http://127.0.0.1:12715/flow/answer', // 应答接口地址
            robotId: robotId, // 机器人id
            mainFlowId: mainFlowId, // 主流程id
            chatHistory: chatRecords.value, // 保存聊天记录数据。这些数据，是在SDK里保存的
        });
    }
    // 发送请求
    await dialogFlowAiSDK.sendMessage({
        type: dialogFlowAiSDK.MessageKind.PLAIN_TEXT,
        content: userAsk.value,
    });
    // 检查是否结束应答
    if (dialogFlowAiSDK.chatHasEnded) {
        dialogFlowAiSDK.addChat('会话结束', 'terminateText', dialogFlowAiSDK.MessageKind.PLAIN_TEXT, -1);
        // 当会话完成，重置SDK
        dialogFlowAiSDK = null;
    }
    waitingResponse.value = false;
}
```
