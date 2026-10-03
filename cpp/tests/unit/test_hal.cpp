#include <iostream>
#include <cassert>
#include <vector>
#include <filesystem>
#include "kinnovel/hal/IDisplay.hpp"
#include "src/hal/mock/MockDisplay.hpp"
#include "src/hal/mock/MockInput.hpp"
#include "src/hal/mock/MockPowerManager.hpp"
#include "kinnovel/core/Logger.hpp"

using namespace kinnovel;
using namespace kinnovel::hal;

void testMockDisplay() {
    std::cout << "[Test] testMockDisplay..." << std::endl;
    MockDisplay display(800, 600);
    assert(display.getWidth() == 800);
    assert(display.getHeight() == 600);
    assert(display.getBpp() == 8);

    // Initial state: white buffer (255)
    assert(display.getBuffer().size() == 800 * 600);
    assert(display.getBuffer()[0] == 255);

    // Write a black 10x10 square at (10, 10)
    std::vector<uint8_t> blackSquare(10 * 10, 0);
    display.writeImage(blackSquare.data(), 10, 10, 10, 10, 10);

    // Check pixel at (10, 10) is 0
    assert(display.getBuffer()[10 * 800 + 10] == 0);
    // Pixel outside remains 255
    assert(display.getBuffer()[9 * 800 + 10] == 255);

    // Refresh count
    Rect r{10, 10, 10, 10};
    display.refresh(r, false, Waveform::Gc16);
    assert(display.getRefreshCount() == 1);
    assert(display.getFlashingRefreshCount() == 0);

    display.refresh(r, true, Waveform::Gc16);
    assert(display.getRefreshCount() == 2);
    assert(display.getFlashingRefreshCount() == 1);

    // Save PNG test
    std::string testPng = "test_display_out.png";
    bool saved = display.saveToPng(testPng);
    assert(saved);
    (void)saved;
    assert(std::filesystem::exists(testPng));
    std::filesystem::remove(testPng);

    std::cout << "[Test] testMockDisplay passed!" << std::endl;
}

void testMockInput() {
    std::cout << "[Test] testMockInput..." << std::endl;
    MockInput input;
    input.initialize(1000, 1000);

    assert(!input.isGrabbed());
    input.grab();
    assert(input.isGrabbed());
    input.ungrab();
    assert(!input.isGrabbed());

    // Test tap
    input.injectTap(500, 250, 50);

    bool received = false;
    input.listen([&](const TouchGesture& g) {
        received = true;
        assert(g.kind == GestureKind::Tap);
        assert(g.point.xPixel == 500);
        assert(g.point.yPixel == 250);
        assert(std::abs(g.point.xRatio - 0.5f) < 0.001f);
        assert(std::abs(g.point.yRatio - 0.25f) < 0.001f);
    });
    assert(received);

    // Test swipe
    input.injectSwipe(GestureKind::Left, 800, 500, 200, 500, 120);
    received = false;
    input.listen([&](const TouchGesture& g) {
        received = true;
        assert(g.kind == GestureKind::Left);
        assert(g.startPoint.xPixel == 800);
        assert(g.endPoint.xPixel == 200);
        assert(g.distancePx >= 40);
    });
    assert(received);

    std::cout << "[Test] testMockInput passed!" << std::endl;
}

class TestObserver : public IPowerObserver {
public:
    void onSuspend() override { suspended = true; }
    void onResume() override { resumed = true; }
    bool suspended = false;
    bool resumed = false;
};

void testMockPower() {
    std::cout << "[Test] testMockPower..." << std::endl;
    MockPowerManager power;
    TestObserver observer;
    power.start(&observer);

    assert(!power.isSleeping());

    power.handlePowerKey(); // Triggers suspend
    assert(power.isSleeping());
    assert(observer.suspended);

    power.handlePowerKey(); // Triggers resume
    assert(!power.isSleeping());
    assert(observer.resumed);

    std::cout << "[Test] testMockPower passed!" << std::endl;
}

int main() {
    std::cout << "=== Running HAL Unit Tests ===" << std::endl;
    testMockDisplay();
    testMockInput();
    testMockPower();
    std::cout << "=== All HAL Tests Passed Successfully! ===" << std::endl;
    return 0;
}
