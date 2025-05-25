use std::collections::HashMap;
#[derive(Clone)]
#[derive(Debug)]
pub struct Language {
    cfg: HashMap<String, String>,
}

impl Language {
    pub fn new() -> Self {
        let mut cfg: HashMap<String, String> = HashMap::new();
        // Инициализация строк на русском языке (как в предоставленном коде)
        cfg.insert("lang_0001".to_string(), "Невозможно создать игру - $GAMENAME$. Попробуйте другое имя".to_string());
        cfg.insert("lang_0002".to_string(), "Ошибка! Игрок $USER$ является Администратором".to_string());
        cfg.insert("lang_0003".to_string(), "$USER$ добавлен в Администраторы".to_string());
        cfg.insert("lang_0004".to_string(), "Ошибка добавления $USER$ в Администраторы".to_string());
        cfg.insert("lang_0005".to_string(), "У вас недостаточно прав для использования данной команды".to_string());
        cfg.insert("lang_0006".to_string(), "Ошибка! Игрок $VICTIM$ уже заблокирован".to_string());
        cfg.insert("lang_0007".to_string(), "Заблокирован игрок $VICTIM$".to_string());
        cfg.insert("lang_0008".to_string(), "Ошибка блокировки игрока $VICTIM$".to_string());
        cfg.insert("lang_0009".to_string(), "Игрок $USER$ повышен до Администратора".to_string());
        cfg.insert("lang_0010".to_string(), "Игрок $USER$ не является Администратором".to_string());
        cfg.insert("lang_0011".to_string(), "Игрок $VICTIM$ заблокирован - $DATE$ администратором - $ADMIN$ по причине - $REASON$".to_string());
        cfg.insert("lang_0012".to_string(), "Игрок $VICTIM$ не заблокирован".to_string());
        cfg.insert("lang_0013".to_string(), "0 Администраторов".to_string());
        cfg.insert("lang_0014".to_string(), "1 Администратор".to_string());
        cfg.insert("lang_0015".to_string(), "$COUNT$ Администраторов".to_string());
        cfg.insert("lang_0016".to_string(), "Нет заблокированных игроков на этом сервере".to_string());
        cfg.insert("lang_0017".to_string(), "1 заблокированный игрок на этом сервере".to_string());
        cfg.insert("lang_0018".to_string(), "$COUNT$ заблокированых игроков на этом сервере".to_string());
        cfg.insert("lang_0019".to_string(), "Вы не можете удалить Главного Администратора".to_string());
        cfg.insert("lang_0020".to_string(), "$USER$ удалён из Администраторов".to_string());
        cfg.insert("lang_0021".to_string(), "Ошибка удаления $USER$ из Администраторов".to_string());
        cfg.insert("lang_0022".to_string(), "Разблокирован игрок $VICTIM$".to_string());
        cfg.insert("lang_0023".to_string(), "Ошибка разблокирования игрока $VICTIM$".to_string());
        cfg.insert("lang_0024".to_string(), "Номер игры $NUMBER$ - $DESCRIPTION$".to_string());
        cfg.insert("lang_0025".to_string(), "Номер игры $NUMBER$ не существует".to_string());
        cfg.insert("lang_0026".to_string(), "Игра {$DESCRIPTION$} уже создана, $CURRENT$ из $MAX$ игр в процессе".to_string());
        cfg.insert("lang_0027".to_string(), "Нет созданных игр, $CURRENT$ из $MAX$ игр в процессе".to_string());
        cfg.insert("lang_0028".to_string(), "Невозможно загрузить файлы конфигурации из внешней папки".to_string());
        cfg.insert("lang_0029".to_string(), "Загрузка файла конфигурации $FILE$".to_string());
        cfg.insert("lang_0030".to_string(), "Невозможно загрузить не существующий фаил конфигурации $FILE$".to_string());
        cfg.insert("lang_0031".to_string(), "Создание приватной игры - $GAMENAME$. Владелец - $USER$".to_string());
        cfg.insert("lang_0032".to_string(), "Создание публичной игры $GAMENAME$. Владелец - $USER$".to_string());
        cfg.insert("lang_0033".to_string(), "Невозможно прервать игру - $DESCRIPTION$. Она уже стартовала, просто подождите несколько секунд".to_string());
        cfg.insert("lang_0034".to_string(), "Прервана игра - $DESCRIPTION$".to_string());
        cfg.insert("lang_0035".to_string(), "Невозможно прервать игру. Нет созданных игр.".to_string());
        cfg.insert("lang_0036".to_string(), "Версия - GHost++ v$VERSION$ :: w3gh.ru/codelain.com".to_string());
        cfg.insert("lang_0037".to_string(), "Версия - GHost++ :: w3gh.ru/codelain.com".to_string());
        cfg.insert("lang_0038".to_string(), "Невозможно создать - $GAMENAME$. В процессе другая игра - $DESCRIPTION$".to_string());
        cfg.insert("lang_0039".to_string(), "Невозможно создать - $GAMENAME$. Максимальное значение ($MAX) созданных игр не может быть превышено".to_string());
        cfg.insert("lang_0040".to_string(), "Игра - $DESCRIPTION$ окончена".to_string());
        cfg.insert("lang_0041".to_string(), "Авторизуйтесь отправив сообщение - /r s".to_string());
        cfg.insert("lang_0042".to_string(), ".".to_string());
        cfg.insert("lang_0043".to_string(), "Подмена ника. Возможно реальный $USER$ ушёл".to_string());
        cfg.insert("lang_0044".to_string(), "Подмена ника. Возможно реальный $USER$ недоступен".to_string());
        cfg.insert("lang_0045".to_string(), "Подмена ника. Возможно реальный $USER$ пишет сообщение".to_string());
        cfg.insert("lang_0046".to_string(), "Подмена ника. Опознан реальный $USER$ не в игре".to_string());
        cfg.insert("lang_0047".to_string(), "Подмена ника. Опознано реальный $USER$ на приватном канале".to_string());
        cfg.insert("lang_0048".to_string(), "Подмена ника. Опознано реальный $USER$ в другой игре".to_string());
        cfg.insert("lang_0049".to_string(), "Старт отменён!".to_string());
        cfg.insert("lang_0050".to_string(), "Заблокированный игрок $VICTIM$ пытается присоединиться в игру".to_string());
        cfg.insert("lang_0051".to_string(), "Невозможно заблокировать - $VICTIM$. Значение не найдено".to_string());
        cfg.insert("lang_0052".to_string(), "Игрок $VICTIM$ получил бан от $USER$".to_string());
        cfg.insert("lang_0053".to_string(), "Невозможно забанить $VICTIM$. Найдено более одного значения".to_string());
        cfg.insert("lang_0054".to_string(), "$USER$ добавлен в список зарезервированных".to_string());
        cfg.insert("lang_0055".to_string(), "Невозможно выкинуть игрока - $VICTIM$. Значение не найдено".to_string());
        cfg.insert("lang_0056".to_string(), "Невозможно выкинуть игрока - $VICTIM$. Найдено более одного значения".to_string());
        cfg.insert("lang_0057".to_string(), "Установка минимум задержки - $MIN$ мс".to_string());
        cfg.insert("lang_0058".to_string(), "Установка максимум задержки - $MAX$ мс".to_string());
        cfg.insert("lang_0059".to_string(), "Установка задержки - $LATENCY$ мс".to_string());
        cfg.insert("lang_0060".to_string(), "Выкинуто $TOTAL$ игроков с пингом выше чем $PING$".to_string());
        cfg.insert("lang_0061".to_string(), "$USER$ - $TOTALGAMES$ игр. Средняя загрузка: $AVGLOADINGTIME$ сек. Среднее пребывание: $AVGSTAY$ %".to_string());
        cfg.insert("lang_0062".to_string(), "$USER$ не играл игр на этом боте".to_string());
        cfg.insert("lang_0063".to_string(), "$VICTIM$ автоматически выкинут из-за высокого пинга - $PING$".to_string());
        cfg.insert("lang_0064".to_string(), "$USER$ опознан".to_string());
        cfg.insert("lang_0065".to_string(), "Игроки ещё не прошли Spoofcheck (Проверка подмены ника) - $NOTSPOOFCHECKED$".to_string());
        cfg.insert("lang_0066".to_string(), "Пройдите проверку написав - /w $HOSTNAME$ s , или подождите несколько секунд для авторизации".to_string());
        cfg.insert("lang_0067".to_string(), "Подтвердите себя написав - /w $HOSTNAME$ s".to_string());
        cfg.insert("lang_0068".to_string(), "Все прошли проверку".to_string());
        cfg.insert("lang_0069".to_string(), "Игроки ещё не пропингованы 3 раза - $NOTPINGED$".to_string());
        cfg.insert("lang_0070".to_string(), "Каждый здесь прошел пинг тест".to_string());
        cfg.insert("lang_0071".to_string(), "Быстрая загрузка $USER$ - $LOADINGTIME$ сек".to_string());
        cfg.insert("lang_0072".to_string(), "Долгая загрузка $USER$ - $LOADINGTIME$ сек".to_string());
        cfg.insert("lang_0073".to_string(), "Ваше время загрузки $LOADINGTIME$ секунд".to_string());
        cfg.insert("lang_0074".to_string(), "$USER$ - $TOTALGAMES$ игр (В/П: $TOTALWINS$/$TOTALLOSSES$). Герои K/D/A: $TOTALKILLS$/$TOTALDEATHS$/$TOTALASSISTS$ ($AVGKILLS$/$AVGDEATHS$/$AVGASSISTS$). Крипы K/D/N: $TOTALCREEPKILLS$/$TOTALCREEPDENIES$/$TOTALNEUTRALKILLS$ ($AVGCREEPKILLS$/$AVGCREEPDENIES$/$AVGNEUTRALKILLS$). T/R/C: $TOTALTOWERKILLS$/$TOTALRAXKILLS$/$TOTALCOURIERKILLS$".to_string());
        cfg.insert("lang_0075".to_string(), "$USER$ не играл DotA игр на этом боте".to_string());
        cfg.insert("lang_0076".to_string(), "освободил место для зарезервированного игрока $RESERVED$".to_string());
        cfg.insert("lang_0077".to_string(), "освободил место для создателя данной игры $OWNER$".to_string());
        cfg.insert("lang_0078".to_string(), "выкинут игроком $USER$".to_string());
        cfg.insert("lang_0079".to_string(), "потерял соединение (ошибка игрока - $ERROR$)".to_string());
        cfg.insert("lang_0080".to_string(), "потерял соединение (ошибка соединения - $ERROR$)".to_string());
        cfg.insert("lang_0081".to_string(), "потерял соединение (соединение закрыто удалённым хостом)".to_string());
        cfg.insert("lang_0082".to_string(), "вышел из игры ДОБРОВОЛЬНО".to_string());
        cfg.insert("lang_0083".to_string(), "Завершение игры - $DESCRIPTION$".to_string());
        cfg.insert("lang_0084".to_string(), "потерял соединение (истёк лимит)".to_string());
        cfg.insert("lang_0085".to_string(), "Глобальный чат ЗАБЛОКИРОВАН".to_string());
        cfg.insert("lang_0086".to_string(), "Глобальный чат РАЗБЛОКИРОВАН".to_string());
        cfg.insert("lang_0087".to_string(), "Произведён разброс игроков".to_string());
        cfg.insert("lang_0088".to_string(), "Невозможно загрузить фаил конфигурации поскольку вы находитесь в лобби. unhost сначала".to_string());
        cfg.insert("lang_0089".to_string(), "Игроки начали загружать карту - $STILLDOWNLOADING$".to_string());
        cfg.insert("lang_0090".to_string(), "Refresh сообщение вкл".to_string());
        cfg.insert("lang_0091".to_string(), "Refresh сообщение выкл".to_string());
        cfg.insert("lang_0092".to_string(), "Как минимум одна игра в процессе. Используйте 'force' для завершения в любом случае".to_string());
        cfg.insert("lang_0093".to_string(), "Текущий конфиг - $MAPCFG$".to_string());
        cfg.insert("lang_0094".to_string(), "отлагал (выкинут Админом)".to_string());
        cfg.insert("lang_0095".to_string(), "отлагал (выкинут Голосованием)".to_string());
        cfg.insert("lang_0096".to_string(), "$USER$ проголосовал ВЫКИНУТЬ".to_string());
        cfg.insert("lang_0097".to_string(), "Уровень задержки - $LATENCY$ мс".to_string());
        cfg.insert("lang_0098".to_string(), "Лимит синхронизации - $SYNCLIMIT$ пакетов".to_string());
        cfg.insert("lang_0099".to_string(), "Установка минимум синхронизации - $MIN$ пакетов".to_string());
        cfg.insert("lang_0100".to_string(), "Установка максимум синхронизации - $MAX$ пакетов".to_string());
        cfg.insert("lang_0101".to_string(), "Установка синхронизации игроков - $SYNCLIMIT$ пакетов".to_string());
        cfg.insert("lang_0102".to_string(), "Невозможно создать игру - $GAMENAME$. Бот не зашёл на battle.net".to_string());
        cfg.insert("lang_0103".to_string(), "Авторизация".to_string());
        cfg.insert("lang_0104".to_string(), "Неправильный пароль (попытка $ATTEMPT$)".to_string());
        cfg.insert("lang_0105".to_string(), "Соединение с battle.net...".to_string());
        cfg.insert("lang_0106".to_string(), "Подключен к battle.net".to_string());
        cfg.insert("lang_0107".to_string(), "Отключен от battle.net".to_string());
        cfg.insert("lang_0108".to_string(), "Авторизация в battle.net".to_string());
        cfg.insert("lang_0109".to_string(), "Battle.net создание игры завершено".to_string());
        cfg.insert("lang_0110".to_string(), "Battle.net ошибка создания игры".to_string());
        cfg.insert("lang_0111".to_string(), "Подсоединение к battle.net серверу - $SERVER$ (Истёк лимит)".to_string());
        cfg.insert("lang_0112".to_string(), "$USER$ загрузил карту за $SECONDS$ сек ($RATE$ Кб/сек)".to_string());
        cfg.insert("lang_0113".to_string(), "Невозможно создать игру - $GAMENAME$. Имя игры слишком длинное (максимум 31 символ)".to_string());
        cfg.insert("lang_0114".to_string(), "Владелец игры - $OWNER$".to_string());
        cfg.insert("lang_0115".to_string(), "Только Владелец игры может использовать игровые команды когда игра ЗАБЛОКИРОВАНА".to_string());
        cfg.insert("lang_0116".to_string(), "Игра ЗАБЛОКИРОВАНА. Только Владелец игры может использовать игровые команды".to_string());
        cfg.insert("lang_0117".to_string(), "Игра РАЗБЛОКИРОВАНА. Все Администраторы могут использовать команды".to_string());
        cfg.insert("lang_0118".to_string(), "Невозможно начать загрузку карты для - $VICTIM$. Значение не найдено".to_string());
        cfg.insert("lang_0119".to_string(), "Невозможно начать загрузку карты для - $VICTIM$. Найдено более 1 значения".to_string());
        cfg.insert("lang_0120".to_string(), "Невозможно установить Владельца игры вы не являетесь Администратором или Владельцем игры. Владелец - $OWNER$".to_string());
        cfg.insert("lang_0121".to_string(), "Невозможно проверить - $VICTIM$. Значение не найдено".to_string());
        cfg.insert("lang_0122".to_string(), "Проверен - $VICTIM$. Админ: $ADMIN$, Владелец: $OWNER$, Подмена ника: $SPOOFED$, Серв: $SPOOFEDREALM$, VIP: $RESERVED$.".to_string());
        cfg.insert("lang_0123".to_string(), "Невозможно проверить - $VICTIM$. Найдено более 1 значения".to_string());
        cfg.insert("lang_0124".to_string(), "Когда игра ЗАБЛОКИРОВАНА эта команда отключена".to_string());
        cfg.insert("lang_0125".to_string(), "Невозможно создать игру - $GAMENAME$. Бот отключен".to_string());
        cfg.insert("lang_0126".to_string(), "Создание новых игр Отключено (если игра в лобби она не отключится автоматически)".to_string());
        cfg.insert("lang_0127".to_string(), "Создание новых игр Включено".to_string());
        cfg.insert("lang_0128".to_string(), "Невозможно создать игру - $GAMENAME$. Фаил конфигурации карты не верный".to_string());
        cfg.insert("lang_0129".to_string(), "Ожидание... Игра начнётся когда наберётся $PLAYERS$ игроков".to_string());
        cfg.insert("lang_0130".to_string(), "Автостарт отключен".to_string());
        cfg.insert("lang_0131".to_string(), "Автостарт включен. Игра начнётся когда наберётся $PLAYERS$ игроков".to_string());
        cfg.insert("lang_0132".to_string(), "Анонс сообщение вкл".to_string());
        cfg.insert("lang_0133".to_string(), "Анонс сообщение выкл".to_string());
        cfg.insert("lang_0134".to_string(), "Авто хостинг вкл".to_string());
        cfg.insert("lang_0135".to_string(), "Авто хостинг выкл".to_string());
        cfg.insert("lang_0136".to_string(), "Невозможно загрузить игру вне этой директории".to_string());
        cfg.insert("lang_0137".to_string(), "Невозможно загрузить игру поскольку игра в лобби".to_string());
        cfg.insert("lang_0138".to_string(), "Загрузка игры - $FILE$".to_string());
        cfg.insert("lang_0139".to_string(), "Невозможно загрузить игру - $FILE$, фаил отсутствует".to_string());
        cfg.insert("lang_0140".to_string(), "Невозможно создать игру - $GAMENAME$. Файл с ошибкой".to_string());
        cfg.insert("lang_0141".to_string(), "Невозможно создать игру - $GAMENAME$. Файл не предназначен для этой карты".to_string());
        cfg.insert("lang_0142".to_string(), "Автосохранение вкл".to_string());
        cfg.insert("lang_0143".to_string(), "Автосохранение выкл".to_string());
        cfg.insert("lang_0144".to_string(), "ОПАСНО! Обнаружена Десинхронизация!".to_string());
        cfg.insert("lang_0145".to_string(), "Невозможно mute/unmute - $VICTIM$. Значение не найдено".to_string());
        cfg.insert("lang_0146".to_string(), "$USER$ закрыл чат игрока - $VICTIM$".to_string());
        cfg.insert("lang_0147".to_string(), "$USER$ открыл чат игрока - $VICTIM$".to_string());
        cfg.insert("lang_0148".to_string(), "Невозможно mute/unmute - $VICTIM$. Найдено более одного значения".to_string());
        cfg.insert("lang_0149".to_string(), "$PLAYER$ сохранил текущую игру".to_string());
        cfg.insert("lang_0150".to_string(), "Обновление внутреннего списка клан игроков c battle.net...".to_string());
        cfg.insert("lang_0151".to_string(), "Обновление внутреннего списка друзей c battle.net...".to_string());
        cfg.insert("lang_0152".to_string(), "$PLAYER$ имеет такой же IP как у - $OTHERS$".to_string());
        cfg.insert("lang_0153".to_string(), "Невозможно стартовать голосование. Другое голосование в процессе".to_string());
        cfg.insert("lang_0154".to_string(), "Невозможно стартовать голосование. Недостаточно игроков в игре чтобы запустить".to_string());
        cfg.insert("lang_0155".to_string(), "Невозможно выкинуть $VICTIM$ по голосованию. Значение не найдено".to_string());
        cfg.insert("lang_0156".to_string(), "Невозможно выкинуть $VICTIM$ по голосованию. Этот игрок зарезирвирован".to_string());
        cfg.insert("lang_0157".to_string(), "Голосование чтобы выкинуть - $VICTIM$, стартовал - $USER$. $VOTESNEEDED$ голосов нужно в течение 60 секунд".to_string());
        cfg.insert("lang_0158".to_string(), "Невозможно выкинуть - $VICTIM$. Найдено более одного значения".to_string());
        cfg.insert("lang_0159".to_string(), "Голосование чтобы выкинуть - $VICTIM$ истекло".to_string());
        cfg.insert("lang_0160".to_string(), "Ошибка при выкидывании - $VICTIM$".to_string());
        cfg.insert("lang_0161".to_string(), "$USER$ проголосовал выкинуть - $VICTIM$. $VOTES$ нужно".to_string());
        cfg.insert("lang_0162".to_string(), "Голосование чтобы выкинуть - $VICTIM$ было отменено".to_string());
        cfg.insert("lang_0163".to_string(), "Голосование чтобы выкинуть - $VICTIM$ истекло".to_string());
        cfg.insert("lang_0164".to_string(), "выкинут голосованием".to_string());
        cfg.insert("lang_0165".to_string(), "Пишите $COMMANDTRIGGER$yes для согласия".to_string());
        cfg.insert("lang_0166".to_string(), "Ожидание старта, пинг игроков - $NOTPINGED$".to_string());
        cfg.insert("lang_0167".to_string(), "будет выкинут через 20 секунд, не прошел проверку".to_string());
        cfg.insert("lang_0168".to_string(), "выкинут, имеет самый низкий рейтинг $SCORE$ из среднего - $AVERAGE$".to_string());
        cfg.insert("lang_0169".to_string(), "$PLAYER$ рейтинг $SCORE$".to_string());
        cfg.insert("lang_0170".to_string(), "Игроки прошедшие рейтинг: $RATED$/$TOTAL$. Увеличение: $SPREAD$".to_string());
        cfg.insert("lang_0171".to_string(), "Ошибка листинга карт".to_string());
        cfg.insert("lang_0172".to_string(), "Карты: $MAPS$".to_string());
        cfg.insert("lang_0173".to_string(), "Карт не найдено".to_string());
        cfg.insert("lang_0174".to_string(), "Ошибка листинга конфигов карт".to_string());
        cfg.insert("lang_0175".to_string(), "Конфиги карт: $MAPCONFIGS$".to_string());
        cfg.insert("lang_0176".to_string(), "Конфигов карт не найдено".to_string());
        cfg.insert("lang_0177".to_string(), "$USER$ завершил загрузку".to_string());
        cfg.insert("lang_0180".to_string(), "Загрузка карт включена".to_string());
        cfg.insert("lang_0181".to_string(), "Загрузка карт включена опционально".to_string());
        cfg.insert("lang_0182".to_string(), "Установка HCL коммандной строки - $HCL$".to_string());
        cfg.insert("lang_0183".to_string(), "Невозможно установить HCL строку, содержит недопустимые символы".to_string());
        cfg.insert("lang_0184".to_string(), "Невозможно установить HCL строку, слишком длинная".to_string());
        cfg.insert("lang_0185".to_string(), "Коммандная HCL строка - $HCL$".to_string());
        cfg.insert("lang_0186".to_string(), "Коммандная HCL строка слишком длинная. Используйте 'force' чтобы стартовать принудительно".to_string());
        cfg.insert("lang_0187".to_string(), "Очистка HCL строки".to_string());
        cfg.insert("lang_0188".to_string(), "Рехост как приватная игра - $GAMENAME$. Подождите, это займёт несколько секунд".to_string());
        cfg.insert("lang_0189".to_string(), "Рехост как публичная игра - $GAMENAME$. Подождите, это займёт несколько секунд".to_string());
        cfg.insert("lang_0190".to_string(), "Рехост прошёл успешно!".to_string());
        cfg.insert("lang_0191".to_string(), "$VICTIM$ забаненый по нику пытается войти в игру".to_string());
        cfg.insert("lang_0192".to_string(), "$VICTIM$ забаненый по IP под ником - $BANNEDNAME$, пытается войти в игру".to_string());
        cfg.insert("lang_0193".to_string(), "$VICTIM$ вошёл под заблокированным ником".to_string());
        cfg.insert("lang_0194".to_string(), "$VICTIM$ вошёл под заблокированным IP под ником - $BANNEDNAME$.".to_string());
        cfg.insert("lang_0195".to_string(), "Игроков в игре #$NUMBER$ - $PLAYERS$".to_string());
        cfg.insert("lang_0196".to_string(), "Ошибка. Валидные серверы - $SERVERS$".to_string());
        cfg.insert("lang_0197".to_string(), "Команда $TEAM$ имеет рейтинг - $SCORE$".to_string());
        cfg.insert("lang_0198".to_string(), "Баланс слотов завершен".to_string());
        cfg.insert("lang_0199".to_string(), "$NAME$ выброшен из-за малого рейтинга - $SCORE$ при среднем $AVERAGE$".to_string());
        cfg.insert("lang_0200".to_string(), "Локальные админ сообщения вкл".to_string());
        cfg.insert("lang_0201".to_string(), "Локальные админ сообщения выкл".to_string());
        cfg.insert("lang_0202".to_string(), "выкинут из-за Десинхронизации".to_string());
        cfg.insert("lang_0203".to_string(), "выкинут из-за низкого рейтинга - $SCORE$".to_string());
        cfg.insert("lang_0204".to_string(), "$NAME$ выкинут из-за низкого рейтинга - $SCORE$".to_string());
        cfg.insert("lang_0205".to_string(), "Перезагрузка файлов конфигурации".to_string());
        cfg.insert("lang_0206".to_string(), "Старт отменён, один или несколько игроков вышли из игры несколько секунд назад".to_string());
        cfg.insert("lang_0207".to_string(), "Невозможно создать игру - $GAMENAME$. Нужно использовать 'enforcesg' перед созданием сохранения".to_string());
        cfg.insert("lang_0208".to_string(), "Невозможно загрузить реплей не из текущей директории".to_string());
        cfg.insert("lang_0209".to_string(), "Загрузка реплея - $FILE$".to_string());
        cfg.insert("lang_0210".to_string(), "Невозможно загрузить реплей - $FILE$, возможно он не существует".to_string());
        cfg.insert("lang_0211".to_string(), "Командный триггер: $TRIGGER$".to_string());
        cfg.insert("lang_0212".to_string(), "Вы не можете завершить эту игру, её владелец - $OWNER$, всё ещё в игре".to_string());
        cfg.insert("lang_0213".to_string(), "Вы не можете завершить эту игру, ёё владелец - $OWNER$, всё ещё в лобби".to_string());
        cfg.insert("lang_0214".to_string(), "автоматически выкинут после $SECONDS$ секунд".to_string());
        cfg.insert("lang_0215".to_string(), "потерял соединение (превышен интервал ожидания), но может перезайти, используя GProxy++".to_string());
        cfg.insert("lang_0216".to_string(), "потерял соединение (ошибка соединения - $ERROR$), но может перезайти, используя GProxy++".to_string());
        cfg.insert("lang_0217".to_string(), "потерял соединение (соединение закрыто удалённым хостом), но может перезайти, используя GProxy++".to_string());
        cfg.insert("lang_0218".to_string(), "Пожалуйста ожидайте переподключения ($SECONDS$ секунд осталось).".to_string());
        cfg.insert("lang_0219".to_string(), "был безвозвратно выкинут с GProxy++".to_string());
        cfg.insert("lang_0220".to_string(), "Игрок - $NAME$ переподключился используя GProxy++!".to_string());

        Language { cfg }
    }

    fn get_string(&self, key: &str) -> String {
        self.cfg.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    fn replace(out: String, placeholder: &str, value: &str) -> String {
        out.replace(placeholder, value)
    }

    pub fn unable_to_create_game_try_another_name(&self, server: &str, gamename: &str) -> String {
        let mut out = self.get_string("lang_0001");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$GAMENAME$", gamename)
    }

    pub fn user_is_already_an_admin(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0002");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn added_user_to_admin_database(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0003");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn error_adding_user_to_admin_database(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0004");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn you_dont_have_access_to_that_command(&self) -> String {
        self.get_string("lang_0005")
    }

    pub fn user_is_already_banned(&self, server: &str, victim: &str) -> String {
        let mut out = self.get_string("lang_0006");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$VICTIM$", victim)
    }

    pub fn banned_user(&self, server: &str, victim: &str) -> String {
        let mut out = self.get_string("lang_0007");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$VICTIM$", victim)
    }

    pub fn error_banning_user(&self, server: &str, victim: &str) -> String {
        let mut out = self.get_string("lang_0008");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$VICTIM$", victim)
    }

    pub fn user_is_an_admin(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0009");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn user_is_not_an_admin(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0010");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn user_was_banned_on_by_because(&self, server: &str, victim: &str, date: &str, admin: &str, reason: &str) -> String {
        let mut out = self.get_string("lang_0011");
        out = Self::replace(out, "$SERVER$", server);
        out = Self::replace(out, "$VICTIM$", victim);
        out = Self::replace(out, "$DATE$", date);
        out = Self::replace(out, "$ADMIN$", admin);
        Self::replace(out, "$REASON$", reason)
    }

    pub fn user_is_not_banned(&self, server: &str, victim: &str) -> String {
        let mut out = self.get_string("lang_0012");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$VICTIM$", victim)
    }

    pub fn there_are_no_admins(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0013"), "$SERVER$", server)
    }

    pub fn there_is_admin(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0014"), "$SERVER$", server)
    }

    pub fn there_are_admins(&self, server: &str, count: &str) -> String {
        let mut out = self.get_string("lang_0015");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$COUNT$", count)
    }

    pub fn there_are_no_banned_users(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0016"), "$SERVER$", server)
    }

    pub fn there_is_banned_user(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0017"), "$SERVER$", server)
    }

    pub fn there_are_banned_users(&self, server: &str, count: &str) -> String {
        let mut out = self.get_string("lang_0018");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$COUNT$", count)
    }

    pub fn you_cant_delete_the_root_admin(&self) -> String {
        self.get_string("lang_0019")
    }

    pub fn deleted_user_from_admin_database(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0020");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn error_deleting_user_from_admin_database(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0021");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn unbanned_user(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0022"), "$VICTIM$", victim)
    }

    pub fn error_unbanning_user(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0023"), "$VICTIM$", victim)
    }

    pub fn game_number_is(&self, number: &str, description: &str) -> String {
        let mut out = self.get_string("lang_0024");
        out = Self::replace(out, "$NUMBER$", number);
        Self::replace(out, "$DESCRIPTION$", description)
    }

    pub fn game_number_doesnt_exist(&self, number: &str) -> String {
        Self::replace(self.get_string("lang_0025"), "$NUMBER$", number)
    }

    pub fn game_is_in_the_lobby(&self, description: &str, current: &str, max: &str) -> String {
        let mut out = self.get_string("lang_0026");
        out = Self::replace(out, "$DESCRIPTION$", description);
        out = Self::replace(out, "$CURRENT$", current);
        Self::replace(out, "$MAX$", max)
    }

    pub fn there_is_no_game_in_the_lobby(&self, current: &str, max: &str) -> String {
        let mut out = self.get_string("lang_0027");
        out = Self::replace(out, "$CURRENT$", current);
        Self::replace(out, "$MAX$", max)
    }

    pub fn unable_to_load_config_files_outside(&self) -> String {
        self.get_string("lang_0028")
    }

    pub fn loading_config_file(&self, file: &str) -> String {
        Self::replace(self.get_string("lang_0029"), "$FILE$", file)
    }

    pub fn unable_to_load_config_file_doesnt_exist(&self, file: &str) -> String {
        Self::replace(self.get_string("lang_0030"), "$FILE$", file)
    }

    pub fn creating_private_game(&self, gamename: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0031");
        out = Self::replace(out, "$GAMENAME$", gamename);
        Self::replace(out, "$USER$", user)
    }

    pub fn creating_public_game(&self, gamename: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0032");
        out = Self::replace(out, "$GAMENAME$", gamename);
        Self::replace(out, "$USER$", user)
    }

    pub fn unable_to_unhost_game_countdown_started(&self, description: &str) -> String {
        Self::replace(self.get_string("lang_0033"), "$DESCRIPTION$", description)
    }

    pub fn unhosting_game(&self, description: &str) -> String {
        Self::replace(self.get_string("lang_0034"), "$DESCRIPTION$", description)
    }

    pub fn unable_to_unhost_game_no_game_in_lobby(&self) -> String {
        self.get_string("lang_0035")
    }

    pub fn version_admin(&self, version: &str) -> String {
        Self::replace(self.get_string("lang_0036"), "$VERSION$", version)
    }

    pub fn version_not_admin(&self, version: &str) -> String {
        Self::replace(self.get_string("lang_0037"), "$VERSION$", version)
    }

    pub fn unable_to_create_game_another_game_in_lobby(&self, gamename: &str, description: &str) -> String {
        let mut out = self.get_string("lang_0038");
        out = Self::replace(out, "$GAMENAME$", gamename);
        Self::replace(out, "$DESCRIPTION$", description)
    }

    pub fn unable_to_create_game_max_games_reached(&self, gamename: &str, max: &str) -> String {
        let mut out = self.get_string("lang_0039");
        out = Self::replace(out, "$GAMENAME$", gamename);
        Self::replace(out, "$MAX$", max)
    }

    pub fn game_is_over(&self, description: &str) -> String {
        Self::replace(self.get_string("lang_0040"), "$DESCRIPTION$", description)
    }

    pub fn spoof_check_by_replying(&self) -> String {
        self.get_string("lang_0041")
    }

    pub fn game_refreshed(&self) -> String {
        self.get_string("lang_0042")
    }

    pub fn spoof_possible_is_away(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0043"), "$USER$", user)
    }

    pub fn spoof_possible_is_unavailable(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0044"), "$USER$", user)
    }

    pub fn spoof_possible_is_refusing_messages(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0045"), "$USER$", user)
    }

    pub fn spoof_detected_is_not_in_game(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0046"), "$USER$", user)
    }

    pub fn spoof_detected_is_in_private_channel(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0047"), "$USER$", user)
    }

    pub fn spoof_detected_is_in_another_game(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0048"), "$USER$", user)
    }

    pub fn count_down_aborted(&self) -> String {
        self.get_string("lang_0049")
    }

    pub fn trying_to_join_the_game_but_banned(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0050"), "$VICTIM$", victim)
    }

    pub fn unable_to_ban_no_matches_found(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0051"), "$VICTIM$", victim)
    }

    pub fn player_was_banned_by_player(&self, server: &str, victim: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0052");
        out = Self::replace(out, "$SERVER$", server);
        out = Self::replace(out, "$VICTIM$", victim);
        Self::replace(out, "$USER$", user)
    }

    pub fn unable_to_ban_found_more_than_one_match(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0053"), "$VICTIM$", victim)
    }

    pub fn added_player_to_the_hold_list(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0054"), "$USER$", user)
    }

    pub fn unable_to_kick_no_matches_found(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0055"), "$VICTIM$", victim)
    }

    pub fn unable_to_kick_found_more_than_one_match(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0056"), "$VICTIM$", victim)
    }

    pub fn setting_latency_to_minimum(&self, min: &str) -> String {
        Self::replace(self.get_string("lang_0057"), "$MIN$", min)
    }

    pub fn setting_latency_to_maximum(&self, max: &str) -> String {
        Self::replace(self.get_string("lang_0058"), "$MAX$", max)
    }

    pub fn setting_latency_to(&self, latency: &str) -> String {
        Self::replace(self.get_string("lang_0059"), "$LATENCY$", latency)
    }

    pub fn kicking_players_with_pings_greater_than(&self, total: &str, ping: &str) -> String {
        let mut out = self.get_string("lang_0060");
        out = Self::replace(out, "$TOTAL$", total);
        Self::replace(out, "$PING$", ping)
    }

    pub fn has_played_games_with_this_bot(&self, user: &str, totalgames: &str, avgloadingtime: &str, avgstay: &str) -> String {
        let mut out = self.get_string("lang_0061");
        out = Self::replace(out, "$USER$", user);
        out = Self::replace(out, "$TOTALGAMES$", totalgames);
        out = Self::replace(out, "$AVGLOADINGTIME$", avgloadingtime);
        Self::replace(out, "$AVGSTAY$", avgstay)
    }

    pub fn hasnt_played_games_with_this_bot(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0062"), "$USER$", user)
    }

    pub fn autokicking_player_for_excessive_ping(&self, victim: &str, ping: &str) -> String {
        let mut out = self.get_string("lang_0063");
        out = Self::replace(out, "$VICTIM$", victim);
        Self::replace(out, "$PING$", ping)
    }

    pub fn spoof_check_accepted_for(&self, server: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0064");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$USER$", user)
    }

    pub fn players_not_yet_spoof_checked(&self, notspoofchecked: &str) -> String {
        Self::replace(self.get_string("lang_0065"), "$NOTSPOOFCHECKED$", notspoofchecked)
    }

    pub fn manually_spoof_check_by_whispering(&self, hostname: &str) -> String {
        Self::replace(self.get_string("lang_0066"), "$HOSTNAME$", hostname)
    }

    pub fn spoof_check_by_whispering(&self, hostname: &str) -> String {
        Self::replace(self.get_string("lang_0067"), "$HOSTNAME$", hostname)
    }

    pub fn everyone_has_been_spoof_checked(&self) -> String {
        self.get_string("lang_0068")
    }

    pub fn players_not_yet_pinged(&self, notpinged: &str) -> String {
        Self::replace(self.get_string("lang_0069"), "$NOTPINGED$", notpinged)
    }

    pub fn everyone_has_been_pinged(&self) -> String {
        self.get_string("lang_0070")
    }

    pub fn shortest_load_by_player(&self, user: &str, loadingtime: &str) -> String {
        let mut out = self.get_string("lang_0071");
        out = Self::replace(out, "$USER$", user);
        Self::replace(out, "$LOADINGTIME$", loadingtime)
    }

    pub fn longest_load_by_player(&self, user: &str, loadingtime: &str) -> String {
        let mut out = self.get_string("lang_0072");
        out = Self::replace(out, "$USER$", user);
        Self::replace(out, "$LOADINGTIME$", loadingtime)
    }

    pub fn your_loading_time_was(&self, loadingtime: &str) -> String {
        Self::replace(self.get_string("lang_0073"), "$LOADINGTIME$", loadingtime)
    }

    pub fn has_played_dota_games_with_this_bot(&self, user: &str, totalgames: &str, totalwins: &str, totallosses: &str, totalkills: &str, totaldeaths: &str, totalcreepkills: &str, totalcreepdenies: &str, totalassists: &str, totalneutralkills: &str, totaltowerkills: &str, totalraxkills: &str, totalcourierkills: &str, avgkills: &str, avgdeaths: &str, avgcreepkills: &str, avgcreepdenies: &str, avgassists: &str, avgneutralkills: &str, avgtowerkills: &str, avgraxkills: &str, avgcourierkills: &str) -> String {
        let mut out = self.get_string("lang_0074");
        out = Self::replace(out, "$USER$", user);
        out = Self::replace(out, "$TOTALGAMES$", totalgames);
        out = Self::replace(out, "$TOTALWINS$", totalwins);
        out = Self::replace(out, "$TOTALLOSSES$", totallosses);
        out = Self::replace(out, "$TOTALKILLS$", totalkills);
        out = Self::replace(out, "$TOTALDEATHS$", totaldeaths);
        out = Self::replace(out, "$TOTALCREEPKILLS$", totalcreepkills);
        out = Self::replace(out, "$TOTALCREEPDENIES$", totalcreepdenies);
        out = Self::replace(out, "$TOTALASSISTS$", totalassists);
        out = Self::replace(out, "$TOTALNEUTRALKILLS$", totalneutralkills);
        out = Self::replace(out, "$TOTALTOWERKILLS$", totaltowerkills);
        out = Self::replace(out, "$TOTALRAXKILLS$", totalraxkills);
        out = Self::replace(out, "$TOTALCOURIERKILLS$", totalcourierkills);
        out = Self::replace(out, "$AVGKILLS$", avgkills);
        out = Self::replace(out, "$AVGDEATHS$", avgdeaths);
        out = Self::replace(out, "$AVGCREEPKILLS$", avgcreepkills);
        out = Self::replace(out, "$AVGCREEPDENIES$", avgcreepdenies);
        out = Self::replace(out, "$AVGASSISTS$", avgassists);
        out = Self::replace(out, "$AVGNEUTRALKILLS$", avgneutralkills);
        out = Self::replace(out, "$AVGTOWERKILLS$", avgtowerkills);
        out = Self::replace(out, "$AVGRAXKILLS$", avgraxkills);
        Self::replace(out, "$AVGCOURIERKILLS$", avgcourierkills)
    }

    pub fn hasnt_played_dota_games_with_this_bot(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0075"), "$USER$", user)
    }

    pub fn was_kicked_for_reserved_player(&self, reserved: &str) -> String {
        Self::replace(self.get_string("lang_0076"), "$RESERVED$", reserved)
    }

    pub fn was_kicked_for_owner_player(&self, owner: &str) -> String {
        Self::replace(self.get_string("lang_0077"), "$OWNER$", owner)
    }

    pub fn was_kicked_by_player(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0078"), "$USER$", user)
    }

    pub fn has_lost_connection_player_error(&self, error: &str) -> String {
        Self::replace(self.get_string("lang_0079"), "$ERROR$", error)
    }

    pub fn has_lost_connection_socket_error(&self, error: &str) -> String {
        Self::replace(self.get_string("lang_0080"), "$ERROR$", error)
    }

    pub fn has_lost_connection_closed_by_remote_host(&self) -> String {
        self.get_string("lang_0081")
    }

    pub fn has_left_voluntarily(&self) -> String {
        self.get_string("lang_0082")
    }

    pub fn ending_game(&self, description: &str) -> String {
        Self::replace(self.get_string("lang_0083"), "$DESCRIPTION$", description)
    }

    pub fn has_lost_connection_timed_out(&self) -> String {
        self.get_string("lang_0084")
    }

    pub fn global_chat_muted(&self) -> String {
        self.get_string("lang_0085")
    }

    pub fn global_chat_unmuted(&self) -> String {
        self.get_string("lang_0086")
    }

    pub fn shuffling_players(&self) -> String {
        self.get_string("lang_0087")
    }

    pub fn unable_to_load_config_file_game_in_lobby(&self) -> String {
        self.get_string("lang_0088")
    }

    pub fn players_still_downloading(&self, stilldownloading: &str) -> String {
        Self::replace(self.get_string("lang_0089"), "$STILLDOWNLOADING$", stilldownloading)
    }

    pub fn refresh_messages_enabled(&self) -> String {
        self.get_string("lang_0090")
    }

    pub fn refresh_messages_disabled(&self) -> String {
        self.get_string("lang_0091")
    }

    pub fn at_least_one_game_active_use_force_to_shutdown(&self) -> String {
        self.get_string("lang_0092")
    }

    pub fn currently_loaded_map_cfg_is(&self, mapcfg: &str) -> String {
        Self::replace(self.get_string("lang_0093"), "$MAPCFG$", mapcfg)
    }

    pub fn lagged_out_dropped_by_admin(&self) -> String {
        self.get_string("lang_0094")
    }

    pub fn lagged_out_dropped_by_vote(&self) -> String {
        self.get_string("lang_0095")
    }

    pub fn player_voted_to_drop_laggers(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0096"), "$USER$", user)
    }

    pub fn latency_is(&self, latency: &str) -> String {
        Self::replace(self.get_string("lang_0097"), "$LATENCY$", latency)
    }

    pub fn sync_limit_is(&self, synclimit: &str) -> String {
        Self::replace(self.get_string("lang_0098"), "$SYNCLIMIT$", synclimit)
    }

    pub fn setting_sync_limit_to_minimum(&self, min: &str) -> String {
        Self::replace(self.get_string("lang_0099"), "$MIN$", min)
    }

    pub fn setting_sync_limit_to_maximum(&self, max: &str) -> String {
        Self::replace(self.get_string("lang_0100"), "$MAX$", max)
    }

    pub fn setting_sync_limit_to(&self, synclimit: &str) -> String {
        Self::replace(self.get_string("lang_0101"), "$SYNCLIMIT$", synclimit)
    }

    pub fn unable_to_create_game_not_logged_in(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0102"), "$GAMENAME$", gamename)
    }

    pub fn admin_logged_in(&self) -> String {
        self.get_string("lang_0103")
    }

    pub fn admin_invalid_password(&self, attempt: &str) -> String {
        Self::replace(self.get_string("lang_0104"), "$ATTEMPT$", attempt)
    }

    pub fn connecting_to_bnet(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0105"), "$SERVER$", server)
    }

    pub fn connected_to_bnet(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0106"), "$SERVER$", server)
    }

    pub fn disconnected_from_bnet(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0107"), "$SERVER$", server)
    }

    pub fn logged_in_to_bnet(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0108"), "$SERVER$", server)
    }

    pub fn bnet_game_hosting_succeeded(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0109"), "$SERVER$", server)
    }

    pub fn bnet_game_hosting_failed(&self, server: &str, gamename: &str) -> String {
        let mut out = self.get_string("lang_0110");
        out = Self::replace(out, "$SERVER$", server);
        Self::replace(out, "$GAMENAME$", gamename)
    }

    pub fn connecting_to_bnet_timed_out(&self, server: &str) -> String {
        Self::replace(self.get_string("lang_0111"), "$SERVER$", server)
    }

    pub fn player_downloaded_the_map(&self, user: &str, seconds: &str, rate: &str) -> String {
        let mut out = self.get_string("lang_0112");
        out = Self::replace(out, "$USER$", user);
        out = Self::replace(out, "$SECONDS$", seconds);
        Self::replace(out, "$RATE$", rate)
    }

    pub fn unable_to_create_game_name_too_long(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0113"), "$GAMENAME$", gamename)
    }

    pub fn setting_game_owner_to(&self, owner: &str) -> String {
        Self::replace(self.get_string("lang_0114"), "$OWNER$", owner)
    }

    pub fn the_game_is_locked(&self) -> String {
        self.get_string("lang_0115")
    }

    pub fn game_locked(&self) -> String {
        self.get_string("lang_0116")
    }

    pub fn game_unlocked(&self) -> String {
        self.get_string("lang_0117")
    }

    pub fn unable_to_start_download_no_matches_found(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0118"), "$VICTIM$", victim)
    }

    pub fn unable_to_start_download_found_more_than_one_match(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0119"), "$VICTIM$", victim)
    }

    pub fn unable_to_set_game_owner(&self, owner: &str) -> String {
        Self::replace(self.get_string("lang_0120"), "$OWNER$", owner)
    }

    pub fn unable_to_check_player_no_matches_found(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0121"), "$VICTIM$", victim)
    }

    pub fn checked_player(&self, victim: &str, ping: &str, from: &str, admin: &str, owner: &str, spoofed: &str, spoofedrealm: &str, reserved: &str) -> String {
        let mut out = self.get_string("lang_0122");
        out = Self::replace(out, "$VICTIM$", victim);
        out = Self::replace(out, "$PING$", ping);
        out = Self::replace(out, "$FROM$", from);
        out = Self::replace(out, "$ADMIN$", admin);
        out = Self::replace(out, "$OWNER$", owner);
        out = Self::replace(out, "$SPOOFED$", spoofed);
        out = Self::replace(out, "$SPOOFEDREALM$", spoofedrealm);
        Self::replace(out, "$RESERVED$", reserved)
    }

    pub fn unable_to_check_player_found_more_than_one_match(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0123"), "$VICTIM$", victim)
    }

    pub fn the_game_is_locked_bnet(&self) -> String {
        self.get_string("lang_0124")
    }

    pub fn unable_to_create_game_disabled(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0125"), "$GAMENAME$", gamename)
    }

    pub fn bot_disabled(&self) -> String {
        self.get_string("lang_0126")
    }

    pub fn bot_enabled(&self) -> String {
        self.get_string("lang_0127")
    }

    pub fn unable_to_create_game_invalid_map(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0128"), "$GAMENAME$", gamename)
    }

    pub fn waiting_for_players_before_auto_start(&self, players: &str, playersleft: &str) -> String {
        let mut out = self.get_string("lang_0129");
        out = Self::replace(out, "$PLAYERS$", players);
        Self::replace(out, "$PLAYERSLEFT$", playersleft)
    }

    pub fn auto_start_disabled(&self) -> String {
        self.get_string("lang_0130")
    }

    pub fn auto_start_enabled(&self, players: &str) -> String {
        Self::replace(self.get_string("lang_0131"), "$PLAYERS$", players)
    }

    pub fn announce_message_enabled(&self) -> String {
        self.get_string("lang_0132")
    }

    pub fn announce_message_disabled(&self) -> String {
        self.get_string("lang_0133")
    }

    pub fn auto_host_enabled(&self) -> String {
        self.get_string("lang_0134")
    }

    pub fn auto_host_disabled(&self) -> String {
        self.get_string("lang_0135")
    }

    pub fn unable_to_load_save_games_outside(&self) -> String {
        self.get_string("lang_0136")
    }

    pub fn unable_to_load_save_game_game_in_lobby(&self) -> String {
        self.get_string("lang_0137")
    }

    pub fn loading_save_game(&self, file: &str) -> String {
        Self::replace(self.get_string("lang_0138"), "$FILE$", file)
    }

    pub fn unable_to_load_save_game_doesnt_exist(&self, file: &str) -> String {
        Self::replace(self.get_string("lang_0139"), "$FILE$", file)
    }

    pub fn unable_to_create_game_invalid_save_game(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0140"), "$GAMENAME$", gamename)
    }

    pub fn unable_to_create_game_save_game_map_mismatch(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0141"), "$GAMENAME$", gamename)
    }

    pub fn auto_save_enabled(&self) -> String {
        self.get_string("lang_0142")
    }

    pub fn auto_save_disabled(&self) -> String {
        self.get_string("lang_0143")
    }

    pub fn desync_detected(&self) -> String {
        self.get_string("lang_0144")
    }

    pub fn unable_to_mute_no_matches_found(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0145"), "$VICTIM$", victim)
    }

    pub fn muted_player(&self, victim: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0146");
        out = Self::replace(out, "$VICTIM$", victim);
        Self::replace(out, "$USER$", user)
    }

    pub fn unmuted_player(&self, victim: &str, user: &str) -> String {
        let mut out = self.get_string("lang_0147");
        out = Self::replace(out, "$VICTIM$", victim);
        Self::replace(out, "$USER$", user)
    }

    pub fn unable_to_mute_found_more_than_one_match(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0148"), "$VICTIM$", victim)
    }

    pub fn player_is_saving_the_game(&self, player: &str) -> String {
        Self::replace(self.get_string("lang_0149"), "$PLAYER$", player)
    }

    pub fn updating_clan_list(&self) -> String {
        self.get_string("lang_0150")
    }

    pub fn updating_friends_list(&self) -> String {
        self.get_string("lang_0151")
    }

    pub fn multiple_ip_address_usage_detected(&self, player: &str, others: &str) -> String {
        let mut out = self.get_string("lang_0152");
        out = Self::replace(out, "$PLAYER$", player);
        Self::replace(out, "$OTHERS$", others)
    }

    pub fn unable_to_vote_kick_already_in_progress(&self) -> String {
        self.get_string("lang_0153")
    }

    pub fn unable_to_vote_kick_not_enough_players(&self) -> String {
        self.get_string("lang_0154")
    }

    pub fn unable_to_vote_kick_no_matches_found(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0155"), "$VICTIM$", victim)
    }

    pub fn unable_to_vote_kick_player_is_reserved(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0156"), "$VICTIM$", victim)
    }

    pub fn started_vote_kick(&self, victim: &str, user: &str, votesneeded: &str) -> String {
        let mut out = self.get_string("lang_0157");
        out = Self::replace(out, "$VICTIM$", victim);
        out = Self::replace(out, "$USER$", user);
        Self::replace(out, "$VOTESNEEDED$", votesneeded)
    }

    pub fn unable_to_vote_kick_found_more_than_one_match(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0158"), "$VICTIM$", victim)
    }

    pub fn vote_kick_passed(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0159"), "$VICTIM$", victim)
    }

    pub fn error_vote_kicking_player(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0160"), "$VICTIM$", victim)
    }

    pub fn vote_kick_accepted_need_more_votes(&self, victim: &str, user: &str, votes: &str) -> String {
        let mut out = self.get_string("lang_0161");
        out = Self::replace(out, "$VICTIM$", victim);
        out = Self::replace(out, "$USER$", user);
        Self::replace(out, "$VOTES$", votes)
    }

    pub fn vote_kick_cancelled(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0162"), "$VICTIM$", victim)
    }

    pub fn vote_kick_expired(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0163"), "$VICTIM$", victim)
    }

    pub fn was_kicked_by_vote(&self) -> String {
        self.get_string("lang_0164")
    }

    pub fn type_yes_to_vote(&self, commandtrigger: &str) -> String {
        Self::replace(self.get_string("lang_0165"), "$COMMANDTRIGGER$", commandtrigger)
    }

    pub fn players_not_yet_pinged_auto_start(&self, notpinged: &str) -> String {
        Self::replace(self.get_string("lang_0166"), "$NOTPINGED$", notpinged)
    }

    pub fn was_kicked_for_not_spoof_checking(&self) -> String {
        self.get_string("lang_0167")
    }

    pub fn was_kicked_for_having_furthest_score(&self, score: &str, average: &str) -> String {
        let mut out = self.get_string("lang_0168");
        out = Self::replace(out, "$SCORE$", score);
        Self::replace(out, "$AVERAGE$", average)
    }

    pub fn player_has_score(&self, player: &str, score: &str) -> String {
        let mut out = self.get_string("lang_0169");
        out = Self::replace(out, "$PLAYER$", player);
        Self::replace(out, "$SCORE$", score)
    }

    pub fn rated_players_spread(&self, rated: &str, total: &str, spread: &str) -> String {
        let mut out = self.get_string("lang_0170");
        out = Self::replace(out, "$RATED$", rated);
        out = Self::replace(out, "$TOTAL$", total);
        Self::replace(out, "$SPREAD$", spread)
    }

    pub fn error_listing_maps(&self) -> String {
        self.get_string("lang_0171")
    }

    pub fn found_maps(&self, maps: &str) -> String {
        Self::replace(self.get_string("lang_0172"), "$MAPS$", maps)
    }

    pub fn no_maps_found(&self) -> String {
        self.get_string("lang_0173")
    }

    pub fn error_listing_map_configs(&self) -> String {
        self.get_string("lang_0174")
    }

    pub fn found_map_configs(&self, mapconfigs: &str) -> String {
        Self::replace(self.get_string("lang_0175"), "$MAPCONFIGS$", mapconfigs)
    }

    pub fn no_map_configs_found(&self) -> String {
        self.get_string("lang_0176")
    }

    pub fn player_finished_loading(&self, user: &str) -> String {
        Self::replace(self.get_string("lang_0177"), "$USER$", user)
    }

    pub fn map_downloads_enabled(&self) -> String {
        self.get_string("lang_0180")
    }

    pub fn map_downloads_conditional(&self) -> String {
        self.get_string("lang_0181")
    }

    pub fn setting_hcl(&self, hcl: &str) -> String {
        Self::replace(self.get_string("lang_0182"), "$HCL$", hcl)
    }

    pub fn unable_to_set_hcl_invalid(&self) -> String {
        self.get_string("lang_0183")
    }

    pub fn unable_to_set_hcl_too_long(&self) -> String {
        self.get_string("lang_0184")
    }

    pub fn the_hcl_is(&self, hcl: &str) -> String {
        Self::replace(self.get_string("lang_0185"), "$HCL$", hcl)
    }

    pub fn the_hcl_is_too_long_use_force_to_start(&self) -> String {
        self.get_string("lang_0186")
    }

    pub fn clearing_hcl(&self) -> String {
        self.get_string("lang_0187")
    }

    pub fn trying_to_rehost_as_private_game(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0188"), "$GAMENAME$", gamename)
    }

    pub fn trying_to_rehost_as_public_game(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0189"), "$GAMENAME$", gamename)
    }

    pub fn rehost_was_successful(&self) -> String {
        self.get_string("lang_0190")
    }

    pub fn trying_to_join_the_game_but_banned_by_name(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0191"), "$VICTIM$", victim)
    }

    pub fn trying_to_join_the_game_but_banned_by_ip(&self, victim: &str, bannedname: &str) -> String {
        let mut out = self.get_string("lang_0192");
        out = Self::replace(out, "$VICTIM$", victim);
        Self::replace(out, "$BANNEDNAME$", bannedname)
    }

    pub fn joined_with_banned_name(&self, victim: &str) -> String {
        Self::replace(self.get_string("lang_0193"), "$VICTIM$", victim)
    }

    pub fn joined_with_banned_ip(&self, victim: &str, bannedname: &str) -> String {
        let mut out = self.get_string("lang_0194");
        out = Self::replace(out, "$VICTIM$", victim);
        Self::replace(out, "$BANNEDNAME$", bannedname)
    }

    pub fn players_in_game_number(&self, number: &str, players: &str) -> String {
        let mut out = self.get_string("lang_0195");
        out = Self::replace(out, "$NUMBER$", number);
        Self::replace(out, "$PLAYERS$", players)
    }

    pub fn valid_servers(&self, servers: &str) -> String {
        Self::replace(self.get_string("lang_0196"), "$SERVERS$", servers)
    }

    pub fn team_has_rating(&self, team: &str, score: &str) -> String {
        let mut out = self.get_string("lang_0197");
        out = Self::replace(out, "$TEAM$", team);
        Self::replace(out, "$SCORE$", score)
    }

    pub fn slot_balance_completed(&self) -> String {
        self.get_string("lang_0198")
    }

    pub fn kicked_due_to_low_rating(&self, name: &str, score: &str, average: &str) -> String {
        let mut out = self.get_string("lang_0199");
        out = Self::replace(out, "$NAME$", name);
        out = Self::replace(out, "$SCORE$", score);
        Self::replace(out, "$AVERAGE$", average)
    }

    pub fn local_admin_messages_enabled(&self) -> String {
        self.get_string("lang_0200")
    }

    pub fn local_admin_messages_disabled(&self) -> String {
        self.get_string("lang_0201")
    }

    pub fn kicked_due_to_desync(&self) -> String {
        self.get_string("lang_0202")
    }

    pub fn kicked_due_to_low_score(&self, score: &str) -> String {
        Self::replace(self.get_string("lang_0203"), "$SCORE$", score)
    }

    pub fn player_kicked_due_to_low_score(&self, name: &str, score: &str) -> String {
        let mut out = self.get_string("lang_0204");
        out = Self::replace(out, "$NAME$", name);
        Self::replace(out, "$SCORE$", score)
    }

    pub fn reloading_configuration_files(&self) -> String {
        self.get_string("lang_0205")
    }

    pub fn count_down_aborted_someone_left(&self) -> String {
        self.get_string("lang_0206")
    }

    pub fn unable_to_create_game_use_enforcesg(&self, gamename: &str) -> String {
        Self::replace(self.get_string("lang_0207"), "$GAMENAME$", gamename)
    }

    pub fn unable_to_load_replays_outside(&self) -> String {
        self.get_string("lang_0208")
    }

    pub fn loading_replay(&self, file: &str) -> String {
        Self::replace(self.get_string("lang_0209"), "$FILE$", file)
    }

    pub fn unable_to_load_replay_doesnt_exist(&self, file: &str) -> String {
        Self::replace(self.get_string("lang_0210"), "$FILE$", file)
    }

    pub fn command_trigger_is(&self, trigger: &str) -> String {
        Self::replace(self.get_string("lang_0211"), "$TRIGGER$", trigger)
    }

    pub fn cant_end_game_owner_is(&self, owner: &str) -> String {
        Self::replace(self.get_string("lang_0212"), "$OWNER$", owner)
    }

    pub fn cant_unhost_game_owner_is(&self, owner: &str) -> String {
        Self::replace(self.get_string("lang_0213"), "$OWNER$", owner)
    }

    pub fn auto_kicked_after_seconds(&self, seconds: &str) -> String {
        Self::replace(self.get_string("lang_0214"), "$SECONDS$", seconds)
    }

    pub fn lost_connection_timeout_gproxy(&self) -> String {
        self.get_string("lang_0215")
    }

    pub fn lost_connection_error_gproxy(&self, error: &str) -> String {
        Self::replace(self.get_string("lang_0216"), "$ERROR$", error)
    }

    pub fn lost_connection_closed_gproxy(&self) -> String {
        self.get_string("lang_0217")
    }

    pub fn waiting_to_reconnect(&self, seconds: &str) -> String {
        Self::replace(self.get_string("lang_0218"), "$SECONDS$", seconds)
    }

    pub fn permanently_kicked_gproxy(&self) -> String {
        self.get_string("lang_0219")
    }

    pub fn player_reconnected_gproxy(&self, name: &str) -> String {
        Self::replace(self.get_string("lang_0220"), "$NAME$", name)
    }
}