#pragma once

#include "kinnovel/network/SignalRClient.hpp"
#include "kinnovel/network/SessionStore.hpp"
#include "kinnovel/core/Config.hpp"

#include <string>
#include <vector>
#include <memory>
#include <mutex>

namespace kinnovel::network {

class ApiClient {
public:
    explicit ApiClient(std::shared_ptr<core::Config> config = nullptr,
                       std::shared_ptr<SessionStore> session = nullptr);
    virtual ~ApiClient() = default;

    void setServer(const std::string& server);
    const std::string& getServer() const { return m_server; }

    std::string getUser() const;
    int getUserId() const;
    bool hasRefreshToken() const;

    std::string login(const std::string& email, const std::string& password);
    std::string refreshAccessToken();
    std::string getAccessToken();
    std::string refreshUser();

    std::string invoke(const std::string& method, const std::string& paramsJson = "{}");

    // Public catalogue
    std::string getBookList(int page = 1, int size = 12, const std::string& keywords = "",
                            const std::string& order = "latest", bool ignoreJapanese = false,
                            bool ignoreAi = false, int categoryId = -1);

    std::string getBookCategories(const std::string& bookType = "Novel");
    std::string getRank(int days = 1);
    std::string getAnnouncementList(int page = 1, int size = 12);
    std::string getAnnouncementDetail(int announcementId);

    // Authenticated catalogue and reading
    std::string getBookInfo(int bookId);
    std::string getBookListByIds(const std::vector<int>& ids, const std::string& bookType = "");
    std::string getBooksBySeries(const std::string& seriesName, int page = 1, int size = 24,
                                const std::string& order = "latest", bool ignoreJapanese = false,
                                bool ignoreAi = false);

    // Reading with offline disk fallback
    std::string getNovelContent(int bookId, int sortNum, const std::string& convert = "");
    std::string saveReadPosition(int bookId, int chapterId, const std::string& xpath = ".");
    std::string getReadHistory();
    std::string clearReadHistory();

    // User and shelf
    std::string getMyInfo();
    std::string getNotifications(int page = 1, int size = 12);
    std::string markNotifications(const std::vector<int>& ids);
    std::string getBookShelf();
    std::string saveBookShelf(const std::string& itemsJson, const std::string& version = "20260921");
    std::string signIn();
    std::string getShop();
    std::string getMyItems();
    std::string buyShopItem(const std::string& key, int quantity = 1);
    std::string getComments(const std::string& commentType, int targetId, int page = 1);

    // Cache prefetch and eviction
    bool prefetchImage(const std::string& imageUrl);
    bool prefetchFont(const std::string& fontUrl);
    void pruneCacheIfNeeded();

    std::shared_ptr<SignalRClient> getHub() const { return m_hub; }
    std::shared_ptr<SessionStore> getSession() const { return m_session; }
    std::shared_ptr<core::Config> getConfig() const { return m_config; }

private:
    std::shared_ptr<core::Config> m_config;
    std::shared_ptr<SessionStore> m_session;
    std::shared_ptr<SignalRClient> m_hub;

    std::string m_server;
    std::string m_visitorId;
    std::mutex m_refreshMutex;

    std::string httpApi(const std::string& path,
                        const std::string& payloadJson = "",
                        const std::string& method = "POST",
                        const std::string& token = "");
};

} // namespace kinnovel::network
