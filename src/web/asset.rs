use std::collections::HashMap;

use std::sync::LazyLock;

pub(crate) static ASSETS_MAP: LazyLock<HashMap<&str, usize>> = LazyLock::new(|| {
    HashMap::from([
        (r"/assets/DialogFlowAiSDK.min.js", 0),
        (r"/assets/inbound-bot-PJJg_rST.png", 1),
        (r"/assets/index-Bb3q3-qP.css", 2),
        (r"/assets/index-BgpdbGOH.js", 3),
        (r"/assets/outbound-bot-EmsLuWRN.png", 4),
        (r"/assets/text-bot-CWb_Poym.png", 5),
        (r"/assets/usedByDialogNodeTextGeneration-DrFqkTqi.png", 6),
        (
            r"/assets/usedByDialogNodeTextGeneration-thumbnail-C1iQCVQO.png",
            7,
        ),
        (r"/assets/usedByLlmChatNode-Bv2Fg5P7.png", 8),
        (r"/assets/usedBySentenceEmbedding-Dmju1hVB.png", 9),
        (
            r"/assets/usedBySentenceEmbedding-thumbnail-DVXz_sh0.png",
            10,
        ),
        (r"/favicon.ico", 11),
        ("/", 12),
        (r"/index.html", 12),
    ])
});
