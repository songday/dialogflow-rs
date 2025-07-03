"use strict";
class DialogFlowAiSDK {
    constructor(options) {
        this.url = options.url;
        this.timeoutSec = options.timeoutSec || 10;
        this.mainFlowId = options.mainFlowId;
        this.robotId = options.robotId;
        this.chatHistory = options.chatHistory || [];
        this.version = 1;
        this.sessionId = options.sessionId || this.newSessionId();
        this.importVariables = [];
        this.chatHasEnded = false;
    }

    VarKind = Object.freeze({
        STRING: 'String',
        NUMBER: 'Number',
    });

    UserInputResult = Object.freeze({
        SUCCESSFUL: 'Successful',
        TIMEOUT: 'Timeout',
    });

    MessageKind = Object.freeze({
        PLAIN_TEXT: 'PlainText',
        RICH_TEXT: 'RichText',
        IMAGE: 'Timeout',
    });

    newSessionId() {
        const d = Date.now().toString();
        return d + Math.random().toString(16);
    }

    genRequestBody(userInput, userInputIntent) {
        const self = this;
        const body = {
            robotId: self.robotId,
            mainFlowId: self.mainFlowId,
            sessionId: this.sessionId,
            userInputResult: null,
            userInput: userInput || "",
            importVariables: self.importVariables.splice(0, self.importVariables.length),
            userInputIntent: userInputIntent
        };
        return body;
    }

    appendImportVariable(name, value, kind) {
        const varKind = this.VarKind[kind];
        if (!varKind) {
            throw new Error(`Invalid variable kind: ${kind}`);
        }
        const variable = {
            varName: name,
            varType: kind,
            varVal: value,
        };
        this.importVariables.push(variable);
    }

    correctData(data) {
        if (!this.url) {
            throw new Error(`Missing parameter: url`);
        }
        if (!this.robotId) {
            throw new Error('Missing parameter: robotId');
        }
        if (!this.mainFlowId) {
            throw new Error('Missing parameter: mainFlowId');
        }
        if (data.sessionId == null)
            throw new Error('Missing parameter: sessionId');
        if (data.userInput == null)
            data.userInput = '';
        if (data.userInputResult == null)
            data.userInputResult = this.chatHistory.length == 0 || data.userInput.length > 0 ? this.UserInputResult.SUCCESSFUL : this.UserInputResult.FAILED;
        if (data.importVariables == null)
            data.importVariables = [];
        if (data.userInputIntent != null && data.userInputIntent == '')
            data.userInputIntent = null;
    }

    addChat(t, tS, aT, idx) {
        if (idx && idx > -1) {
            if (idx >= this.chatHistory.length) {
                for (let i = this.chatHistory.length; i < idx; i++) {
                    this.chatHistory.push({
                        id: 'chat-' + Math.random().toString(16),
                        text: '',
                        textSource: tS,
                        answerType: aT,
                    });
                }
            } else {
                this.chatHistory[idx].text += t;
                return idx;
            }
        }
        this.chatHistory.push({
            id: 'chat-' + Math.random().toString(16),
            text: t.trimStart(),
            textSource: tS,
            answerType: aT,
        });
        return this.chatHistory.length - 1;
    }

    appendAnswers(r, idx) {
        console.log(r);
        if (r.status == 200) {
            console.log('data.nextAction:', r.data.nextAction);
            const data = r.data;
            const answers = data.answers;
            let newIdx = -1;
            if (answers != null) {
                for (let i = 0; i < answers.length; i++)
                    newIdx = this.addChat(answers[i].content, 'responseText', answers[i].contentType, idx);
            }
            if (data.nextAction === 'Terminate')
                this.chatHasEnded = true;
            return { chatIdx: newIdx };
        } else {
            throw new Error(`Error: ${r.err.message}`);
        }

    }

    async sendMessage(message) {
        const self = this;

        // 构造请求体
        const body = self.genRequestBody(message.content, null);
        self.correctData(body);

        if (message.content)
            self.addChat(message.content, 'userText', message.type, -1);
        // const res = {
        //     type: self.MessageKind.PLAIN_TEXT,
        //     content: '......',
        //     from: 'bot',
        //     timestamp: new Date().toISOString()
        // };
        // self.chatHistory.push(res);

        var controller = new AbortController();
        var timeoutId = setTimeout(function () {
            controller.abort();
        }, self.timeoutSec * 1000);

        console.log('Request body:', JSON.stringify(body));

        const response = await fetch(self.url, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify(body),
            signal: controller.signal
        });

        clearTimeout(timeoutId);
        console.log('Response:', response);
        if (!response.ok) throw new Error('Network response was not ok');
        const contentType = response.headers.get('content-type') || '';

        const isStream = contentType.includes('text/event-stream') ||
            contentType.includes('application/x-ndjson') ||
            contentType.includes('text/plain');

        if (isStream) {
            const reader = response.body.getReader();
            const decoder = new TextDecoder('utf-8');
            const stream = new ReadableStream({
                async start(controller) {
                    while (true) {
                        const { done, value } = await reader.read();
                        if (done) break;
                        controller.enqueue(decoder.decode(value, { stream: true }));
                    }
                    controller.close();
                }
            });

            // ReadableStream -> text chunks
            const textStream = stream.getReader();

            let { value, done } = await textStream.read();
            let idx = -1;
            while (!done) {
                console.log('chunk:', value);
                // console.log('idx:', idx);
                if (value === null || value === undefined || value.trim().length == 0) {
                    continue;
                }
                value.substring(1, value.length - 1).split('}{').forEach((line) => {
                    if (line.trim().length > 0) {
                        console.log('line:', line);
                        // const c = value.charAt(0);
                        // let j;
                        // if (c !== '{' && c !== '[') {
                        //     j = { data: { answers: [{ content: value }] } };
                        // }
                        // else
                        //     j = JSON.parse(line);
                        const j = JSON.parse('{' + line + '}');
                        if (Object.hasOwn(j, 'contentSeq') && j.contentSeq !== null) {
                            self.appendAnswers({ status: 200, data: { answers: [{ content: j.content }] } }, j.contentSeq);
                        } else {
                            const r = self.appendAnswers({ status: 200, data: JSON.parse(j.content) }, idx);
                            idx = r.chatIdx;
                        }
                    }
                });
                ({ value, done } = await textStream.read());
            }
        } else {
            const res = await response.json();
            console.log('Response data:', res);
            self.appendAnswers(res, -1);
        }
    };
}

export { DialogFlowAiSDK };
