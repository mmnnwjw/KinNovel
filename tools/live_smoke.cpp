#include <iostream>
#include <fstream>
#include <string>
#include <chrono>
#include <thread>
#include <filesystem>

#include "kinnovel/network/ApiClient.hpp"
#include "kinnovel/core/Config.hpp"
#include "kinnovel/core/Utils.hpp"
#include "kinnovel/network/HttpTransport.hpp"

using namespace kinnovel::network;
using namespace kinnovel::core;

int main() {
    std::string accPath = "/data/data/com.termux/files/home/testaccount";
    if (!std::filesystem::exists(accPath)) {
        accPath = "testaccount";
    }
    if (!std::filesystem::exists(accPath)) {
        std::cout << "[INFO] testaccount file not found, skipping live smoke test." << std::endl;
        return 0;
    }

    std::ifstream in(accPath);
    std::string email, password;
    if (!(in >> email >> password) || email.find('@') == std::string::npos) {
        std::cerr << "[ERROR] Invalid testaccount file format" << std::endl;
        return 1;
    }

    HttpTransport::globalInit();

    auto cfg = std::make_shared<Config>();
    cfg->set("api_server", "https://api.lightnovel.life");
    cfg->set("strict_tls", false);

    ApiClient api(cfg);

    std::cout << "[SMOKE] Connecting to live server: " << api.getServer() << "..." << std::endl;

    try {
        std::cout << "[SMOKE] Attempting login..." << std::endl;
        std::string userInfo = api.login(email, password);
        std::cout << "[SMOKE] Login successful! User ID: " << api.getUserId() << std::endl;

        std::cout << "[SMOKE] Waiting 10s to obey rate limit..." << std::endl;
        std::this_thread::sleep_for(std::chrono::seconds(10));

        std::cout << "[SMOKE] Fetching announcement list..." << std::endl;
        std::string annList = api.getAnnouncementList(1, 4);
        std::cout << "[SMOKE] Announcement list fetched: " << annList.substr(0, 100) << "..." << std::endl;

        std::cout << "[SMOKE] Waiting 10s to obey rate limit..." << std::endl;
        std::this_thread::sleep_for(std::chrono::seconds(10));

        std::cout << "[SMOKE] Fetching book shelf..." << std::endl;
        std::string shelf = api.getBookShelf();
        std::cout << "[SMOKE] Book shelf fetched successfully!" << std::endl;

        std::cout << "[SMOKE] ALL LIVE ACCOUNT SMOKE TESTS PASSED!" << std::endl;
    } catch (const std::exception& exc) {
        std::cerr << "[SMOKE ERROR] Live test failed: " << exc.what() << std::endl;
        HttpTransport::globalCleanup();
        return 1;
    }

    HttpTransport::globalCleanup();
    return 0;
}
