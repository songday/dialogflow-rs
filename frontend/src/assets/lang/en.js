export default {
    lang: {
        common: {
            add: 'Add',
            back: 'Back',
            save: 'Save',
            saved: 'Successfully saved',
            cancel: 'Cancel',
            edit: 'Edit',
            del: 'Delete',
            deleted: 'Successfully deleted',
            insert: 'Insert',
            nodeName: 'Node name',
            else: 'Otherwise',
            successTip: 'Success',
            errTip: 'Error',
            toDetail: 'View detail',
        },
        err: {},
        mainflow: {
            title: 'Dialog flow list',
            add: 'Add a new dialog flow',
            table: ['Dialog flow name', 'Enabled', 'Operations'],
            form: {
                title: 'Add a new dialog flow',
                name: 'Dialog flow name',
            },
            delConfirm: 'Are you sure you want to delete this dialog flow?',
        },
        flow: {
            nodes: ['DialogNode', 'ConditionNode', 'CollectNode', 'GotoNode'],
            nodesDesc: ['Returns the dialog text to the user', 'Setting up conditions to control where the dialog flow goes', 'Capture user input and save it to a variable', 'Jumping between dialog flows, or to an external link'],
            title: 'Compose dialog flow',
            steps: ['First step: publish flows', 'Second step: testing'],
            save: 'Save current sub-flow',
            pub: 'Publish all sub-flows',
            test: 'Testing dialog flow',
            addSubFlow: 'Add sub-flow',
            form: {
                name: 'Sub-flow name',
            },
            subFlowReleased: 'Successfully released',
            needOne: 'Keep at least one sub-flow',
            delConfirm: 'Are you sure you want to delete this sub-flow?',
            send: 'Send',
            reset: 'Reset',
            changeSaveTip: 'The flow has been modified, do you need to save it and switch?',
            guideReset: 'The conversation is over, if you want to start over, please click the button below to restart',
        },
        dialogNode: {
            nodeName: 'Dialog node',
            nextSteps: ['Waiting for user response', 'Goto next node'],
            errors: ['Node name not filled in', 'Text not filled in', 'Text was too long'],
            form: {
                label: 'Text',
                addVar: 'Insert a variable',
                nextStep: 'Next step',
                choose: 'Select the action to be performed',
            },
            var: {
                title: 'Select a variable to be inserted',
                choose: 'Select a variable',
            }
        },
        conditionNode: {
            types: ['User intent', 'User input', 'Variable'],
            compares: ['Equals', 'NotEquals', 'Contains', 'Timeout'],
            nodeName: 'Condition node',
            errors: ['Condition name not filled in', 'Duplicate condition name', 'Node name not filled in', 'Branches were missing'],
            newBranch: 'Add a new branch',
            condName: 'Name',
            condType: 'Condition',
            condTypePH: 'Select a condition type',
            comparedPH: 'Select the data to be compared',
            compareTypePH: 'Select the type of comparison',
            targetPH: 'Select a value for comparison',
            andCond: '"AND" condition',
            orCond: '"OR" condition',
            newCond: 'Conditions',
        },
        collectNode: {
            nodeName: 'Collection node',
            cTypes: ['User input', 'Number', 'Customize Regular Expression'],
            branches: ['Successful', 'Failure'],
            errors: ['Node name not filled in', 'Collection type not choosed', 'Saving variable not choosed', 'Branches were missing'],
            cTypeName: 'Collection type',
            varName: 'Assignment variable',
            labels: ['Collection type', 'Choose a collection type', 'Customize RegEx', 'Assignment variable', 'Choose a variable'],
        },
        gotoNode: {
            types: ['Conclusion of dialogues', 'Goto another flow', 'Goto sub-flow', 'External link'],
            nodeName: 'Goto node',
            errors: ['Node name not filled in', 'No goto type selected', 'Sub-flow not selected for jumping', 'No external link to fill in'],
            briefs: ['Goto type', 'Conclusion of dialogues', 'Goto sub-flow', 'External link', 'Goto another dialog flow'],
            gotoType: 'Goto type',
            gotoTypePH: 'Select a goto type',
            gotoMainFlow: 'Goto dialog flow',
            gotoMainFlowPH: 'Select a go-to dialog flow',
            gotoSubFlow: 'Goto sub-flow',
            gotoSubFlowPH: 'Select a goto sub-flow',
            externalLink: 'External link',
        },
        intent: {
            title: 'Intent management',
            add: 'Add new intent',
            delConfirm: 'Are you sure you want to delete this intent?',
            table: ['Name', 'Number of keywords', 'Number of regex', 'Number of similar sentences', 'Operations'],
            form: {
                title: 'Add new intent',
                name: 'Name',
            },
            detail: {
                edit: 'Edit intent',
                kw: 'Keywords',
                addKw: 'Add keyword',
                re: 'Regular expressions',
                addRe: 'Add regex',
                sp: 'Similar sentences',
                addSp: 'Add sentence'
            },
        },
        settings: {
            title: 'Settings',
            ipNote: 'At startup, you can override the configured parameters with -ip and -port.',
            ipNote2: 'If the configured IP address is invalid and the application fails to start, use -rs parameter to reset the configuration.',
            prompt2: 'Listening port',
            prompt2_2: 'When the set port is occupied, automatically randomly select other available ports',
            prompt3: 'Max session idle',
            prompt4: 'Minutes',
            note: 'Modified IP address or ports require a reboot to take effect',
            invalidIp: 'Incorrectly set IP address',
        },
        var: {
            types: ['String', 'Number'],
            sources: ['Import', 'Collect', 'External HTTP'],
            title: 'Variables management',
            add: 'Add new variable',
            table: ['Name', 'Type', 'Source of variable value', 'Operations'],
            form: {
                title: 'New Variable',
                name: 'Name',
                type: 'Type',
                choose1: 'Please choose a type',
                source: 'Value source',
                choose2: 'Please choose a source',
            },
        },
        home: {
            title: 'Dialog Flow Visual Editor and Responsing System',
            subTitle: 'Low code dialog flow responsing system',
            btn1: 'Getting Started',
            btn2: 'View docs',
            btn3: 'Try demos',
            dlTitle: 'Download',
            dl1: 'You can download the latest releases on Github',
            dl2: 'If you have any issues or feedback, please email to: dialogflow(AT)yeah.net or create an <a href="https://github.com/easyflow-ai/easyflow/issues">issue</a>',
            introTitle: 'What is this?',
            intro1: 'It\'s similar to Google\'s Dialogflow, but with an additional canvas for editing processes. It\'s also similar to Typebot, but it includes a full answering backend.',
            intro2: 'It has a feature called flow canvas that allows you to visually edit a conversation flow, test it, and finally publish it to the public.',
            intro3: 'Currently, it can return discourse to the user and capture user input, and can also execute different nodes through conditional judgment .',
            intro4: 'It is very small. The entire software, including front-end and back-end, is less than 6M in size, very easy to distribute and deploy.',
            intro5: 'You can modify the listening port of the software so that you can run multiple instances on the same server at the same time to handle different user scenarios.',
            intro6: 'Once it is downloaded, it can be used directly without installing any dependencies. And the data is stored locally, which can protect the data camp privacy.',
            midTitle: 'Advantages',
            adv1Title: 'Easy to use',
            adv1: 'Simple and intuitive.<br />Everybody can use it<br />Just few drag drop and clicks<br />A dialog flow can then be mapped out',
            demo: 'Try Demos',
            demo1: 'Notification of loan repayment',
            demo2: 'Information collection',
            demo3: 'Notification',
            demoUnvailableTitle: 'Demos are not available on Github',
            demoUnvailableContent: 'Since there is currently no server to host the backend.<br />But you can <a href="https://github.com/easyflow-ai/easyflow/releases">download</a> this software and try these 3 demonstration dialog flows',
            adv2Title: 'Tiny fast and portable',
            adv2: 'Only ONE executable file (database is generated automatically)<br />pretty easy for deployment<br />Relying on AoT compilation technology<br />Program provides high concurrency and blazingly fast responses',
            adv3Title: 'Deal with various issues',
            adv3: 'Use different combinations of flow nodes<br />Meet the business requirements of different scenarios<br />Solve problems encountered by different groups of people',
            adv4Title: 'Compatibilities',
            adv4: 'Front-end support for Firefox, Chrome, Microsoft Edge, Safari, Opera<br />and other major browsers<br /><br />The application supports deployment on Linux, Windows Server, macOS Server<br />and other operating systems.',
            adv5Title: 'Easy to integrate',
            adv5: 'Provides a response interface based on the HTTP protocol<br/ ><br />FreeSwitch can also be integrated to enable intelligent speech robots',
            adv5Doc: 'View docs',
        },
        guide: {
            title1: 'Dialog flows',
            nav1: 'Click here to create new dialog flow or edit existing flow',
            title2: 'Intentions',
            nav2: 'Intents management',
            desc2: 'Intent, used to summarize user input, what purpose does it belong to.',
            title3: 'Variables',
            nav3: 'Variables management',
            desc3: 'Variables are used to store user input, or external data from HTTP, which is used in the answer of the dialog flow, conditional judgment.',
            title4: 'Robot settings',
            nav4: 'Modify Configurations',
            desc4: 'Modify the listening IP and port.',
            title5: 'Operation manual and integration documentation',
            nav5: 'View documents',
            desc5: 'Understand how to quickly build a dialog flow through the canvas. Learn how to connect to the answering interface through code',
        },
    }
}