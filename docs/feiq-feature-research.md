# 飞秋（FeiQ）局域网聊天软件功能清单（资料梳理）

调研时间：2026-07-31。以下以飞秋工作室的官方站点为主；官方页面标注的最新桌面正式版是 **飞秋 2013（2013-06-06）**，因此功能应理解为该历史版本及其 2012/2013 测试版能力，不代表今天仍维护或所有版本都具备。

## 核心定位与网络特征

- 局域网即时通信和文件传输，绿色、免费、免服务器；基于 TCP/IP（UDP），兼容飞鸽传书（IPMSG）协议，使用 TCP/UDP 2425 端口。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)。
- 不依赖互联网服务器即可发现局域网用户、聊天和建群；支持多网卡、多网段刷新/获取好友。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程（2012-07-16/2012-09-07）](http://www.feiq18.com/config_nav.php?id=18)。

## 消息与聊天

- 单聊文字消息；支持文字、图片、GIF 动画、表情、自定义表情、截图和随手涂鸦；可多人群发/多人对话。来源：[官方首页功能总览](http://www.feiq18.com/)、[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)。
- 离线消息/文件：对方离线时可发送，待对方上线后自动发送（2013 RC 新增/强化）。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 群聊天：无需服务器建群；可无限创建群、搜索群、群公告、群成员刷新、群消息设置。来源：[官方首页](http://www.feiq18.com/)、[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程（2013 RC）](http://www.feiq18.com/config_nav.php?id=18)。
- 聊天记录：保存、查看最近消息、全部记录检索；2013 版本优化读取/显示速度和打开会话时的最近消息。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)。
- 消息治理：黑名单、垃圾信息屏蔽、勿扰模式；可按组屏蔽信息或对组成员隐身，并有上下线通知。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 用户检索与资料：按用户名、组名、IP 搜索，中文支持汉字或拼音首字母；备注名、头像、形象照片、个性签名、联系方式/个人资料共享；可查看/比较好友数、版本并诊断在线状态。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 输入编辑：复制粘贴格式兼容 Word、Excel、QQ 和网页；可右键复制文件/文件夹后粘贴发送。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程（2012-09-07）](http://www.feiq18.com/config_nav.php?id=18)。

## 文件传输与共享

- 点对点高速传文件，支持文件和文件夹、长文件名、4GB 以上大文件；发送/接收双方可看进度。可限制速度、显示剩余时间、取消发送。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方首页](http://www.feiq18.com/)。
- 断点续传：网络中断后继续；2013 版本对已传过的大文件支持重新接收时“秒传”。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)。
- 文件共享下载：主动共享软件/文件或 Windows 共享目录；支持密码、密码提示问题、部分文件免密；可查看下载次数，群共享也可用。来源：[官方首页](http://www.feiq18.com/)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 接收管理：自动接收（可定义目录、重名自动改名）、全部接收/另存；接收后提供打开文件、打开目录、删除等链接；文件监视面板可重新发送。来源：[官方历程](http://www.feiq18.com/config_nav.php?id=18)。

## 协作、扩展与个性化

- 飞秋空间日志：不依赖互联网的局域网博客/个人主页，可发布文字、图片、文件共享，支持评论、浏览记录、权限和模板。来源：[官方首页](http://www.feiq18.com/)、[官方历程（2013 RC）](http://www.feiq18.com/config_nav.php?id=18)。
- 应用管理器：管理/快捷打开本机应用，提供飞秋插件下载平台；历史版本支持插件二次开发。来源：[官方首页](http://www.feiq18.com/)、[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 远程协助/共享桌面/远程维护；语音聊天（语音对话）。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)。
- 日程安排与记事提醒：年/月/周/日/时/分/秒及复杂周期提醒；可执行提示窗口、音乐、指定程序、关机等动作；支持导入导出。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 个性化界面：自定义头像/形象/签名，修改字体，换肤、皮肤包、透明度和 XML+图片界面美化；Unicode、Win7（官方称 Win8 测试版也可用）。来源：[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程（2012-07-16）](http://www.feiq18.com/config_nav.php?id=18)。
- 数据管理：备份/还原私人数据、外观/网络配置、个性信息、聊天记录和日志；导入导出群信息。来源：[官方首页](http://www.feiq18.com/)、[官方产品介绍](http://www.feiq18.com/config_nav.php?id=5)、[官方历程](http://www.feiq18.com/config_nav.php?id=18)。
- 第三方接口与自动化：命令行发送消息/图片/文件、导出好友列表、发表日志；飞秋机器人可按消息调用页面并自动回传结果；可打开/搜索局域网 Windows 共享文件夹。来源：[官方历程（2013-03-23）](http://www.feiq18.com/config_nav.php?id=18)。
- 其他桌面便利：最近联系人面板、天气显示、桌面聊天快捷方式、老板键/截图快捷键、单实例唤回、自动更新，以及端口被占用时的进程诊断。来源：[官方历程](http://www.feiq18.com/config_nav.php?id=18)。

## 版本范围与使用时的注意

- 官网首页下载链接为 `feiq.zip`，页面注明“飞秋 2013 正式版”、更新日期 2013-06-06，并自称官方唯一网站：[首页](http://www.feiq18.com/)。
- 许多能力是 2012/2013 测试版逐步加入（如断点续传、空间日志、群共享、命令行接口、机器人）；旧版飞鸽兼容、群图片接收、不同网段发现等在历程页有多次修复记录：[版本历程](http://www.feiq18.com/config_nav.php?id=18)。部署或复刻时应明确目标版本并把“官方宣传功能”和“特定版本新增/修复”分开。

