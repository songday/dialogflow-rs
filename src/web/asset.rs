use std::collections::HashMap;

use std::sync::LazyLock;

pub(crate) static ASSETS_MAP: LazyLock<HashMap<&str, usize>> = LazyLock::new(|| {
HashMap::from([
(r"/assets/DialogFlowAiSDK.min.js", 0),
(r"/assets/inbound-bot-PJJg_rST.png", 1),
(r"/assets/index-BgHOwAJS.js", 2),
(r"/assets/index-C1HBSe1j.css", 3),
(r"/assets/index-CBXDnolR.css", 4),
(r"/assets/index-HTbpkmSB.js", 5),
(r"/assets/outbound-bot-EmsLuWRN.png", 6),
(r"/assets/text-bot-CWb_Poym.png", 7),
(r"/assets/usedByDialogNodeTextGeneration-DrFqkTqi.png", 8),
(r"/assets/usedByDialogNodeTextGeneration-thumbnail-C1iQCVQO.png", 9),
(r"/assets/usedByLlmChatNode-Bv2Fg5P7.png", 10),
(r"/assets/usedBySentenceEmbedding-Dmju1hVB.png", 11),
(r"/assets/usedBySentenceEmbedding-thumbnail-DVXz_sh0.png", 12),
(r"/favicon.ico", 13),
("/", 14),
(r"/index.html", 14),
])});
