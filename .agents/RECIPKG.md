# Recipkg 项目接入

主输入是 recipes/cortexfs/；保留项目名 cortexfs，不新建 -recipkg 仓库。
当前没有实现源码；生成实现需要独立授权，并放到 generated/<源码仓库名>/ 固定子模块。

## 共享技能固定提交

这些是各自 canonical recipkg 上游仓库的真实 Git 子模块，不是复制的技能或项目 fork。
2026-10-29 已核对远程、HEAD、跟踪文件、UTF-8 SKILL.md 和本地引用资源；
父索引 gitlink 必须为 160000，子模块保持干净。原有 CortexFS/Speckit 自定义技能保留。

| 技能 | 固定 commit |
| --- | --- |
| [add-dependency](skills/add-dependency/SKILL.md) | `372ccd8bb4c6e5b35ebefbad41a513e8217527c6` |
| [build-software-package](skills/build-software-package/SKILL.md) | `ae331d94efddfb3c3caec9761785bbfa5606b3cd` |
| [discover-dependencies](skills/discover-dependencies/SKILL.md) | `7276d82058494a2d7a012b3d831b2eef36d04f09` |
| [manage-git-repository](skills/manage-git-repository/SKILL.md) | `96cd5fa8240b193cf077e778e97e8b7323e6e4ff` |
| [migrate-to-recipkg](skills/migrate-to-recipkg/SKILL.md) | `b120b393696e512e67451c63c40b6925f98c565c` |
| [prompt-to-software](skills/prompt-to-software/SKILL.md) | `05f6687288e18272391765d0d6b393fc6684aa18` |
| [rewrite-software](skills/rewrite-software/SKILL.md) | `e8916e28b0748c71c7dec1f78c38351b4f16fb9e` |
| [rewrite-to-recipkg](skills/rewrite-to-recipkg/SKILL.md) | `51ad3642d6cbf220dd6fd325de0efed979ff20a1` |
| [software-to-prompt](skills/software-to-prompt/SKILL.md) | `619648c261ad4fbeffb31e7f6d255d9dd7f27850` |
| [test-feedback](skills/test-feedback/SKILL.md) | `603cc158ca11154557bc7f9db607bb1561625449` |
| [update-recipkg-in-project](skills/update-recipkg-in-project/SKILL.md) | `334b30935f2a4caa7745d4d1b75824b134203c04` |

更新先审查独立上游提交及本地 dirty state，再显式更新父仓库 pin；不自动追 main、
不覆盖项目自定义技能。技能工作区及 Git 元数据绝不放入 recipe archive。

## 本地验证与交付

执行 recipkg extract . --lang zh-CN 获取观察清单；实际检查固定历史对象和文档，
不能只打印任务。更改配方后检查所有三个 recipe，临时打包根配方、检查归档、
渲染完整 recursive build prompt，并核对每份 Markdown 都进入上下文。临时归档和
prompt 结束后删除；无源码恢复、真实运行或独立重建时不得宣称产品通过。

在任务分支提交和推送，通过 PR 交付，不直接推 main、强推或合并。GitHub 仓库发布
配方必须保留既有 topics 并添加 recipkg，再读取确认；topic 不是有效性或可信度证明。

旧 docs/cortexfs-prompts/ 仅保留本机观察材料，其失效远程已从 .gitmodules 移除；
不删除这份本地目录及原 .git/modules 恢复材料。
