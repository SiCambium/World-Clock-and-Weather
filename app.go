package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	minPlaces = 2
	maxPlaces = 10
)

// Place is a saved location the user is tracking.
type Place struct {
	ID        string  `json:"id"`
	Name      string  `json:"name"`
	Admin1    string  `json:"admin1"`
	Country   string  `json:"country"`
	Timezone  string  `json:"timezone"`
	Latitude  float64 `json:"latitude"`
	Longitude float64 `json:"longitude"`
}

// GeocodeResult is a candidate returned from a city search.
type GeocodeResult struct {
	Name      string  `json:"name"`
	Admin1    string  `json:"admin1"`
	Country   string  `json:"country"`
	Timezone  string  `json:"timezone"`
	Latitude  float64 `json:"latitude"`
	Longitude float64 `json:"longitude"`
}

// WeatherData is current + today's weather for a place.
type WeatherData struct {
	Temperature  float64 `json:"temperature"`
	ApparentTemp float64 `json:"apparentTemp"`
	Humidity     float64 `json:"humidity"`
	WindSpeed    float64 `json:"windSpeed"`
	WeatherCode  int     `json:"weatherCode"`
	IsDay        bool    `json:"isDay"`
	TodayHigh    float64 `json:"todayHigh"`
	TodayLow     float64 `json:"todayLow"`
	UpdatedAt    string  `json:"updatedAt"`
}

// PlaceWeather bundles a place with its latest weather.
type PlaceWeather struct {
	Place   Place       `json:"place"`
	Weather WeatherData `json:"weather"`
}

// App struct
type App struct {
	ctx        context.Context
	configPath string
	httpClient *http.Client
}

// NewApp creates a new App application struct
func NewApp() *App {
	return &App{
		httpClient: &http.Client{Timeout: 15 * time.Second},
	}
}

// startup is called when the app starts. The context is saved
// so we can call the runtime methods
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx

	dir, err := os.UserConfigDir()
	if err != nil {
		dir = "."
	}
	appDir := filepath.Join(dir, "TimeAtlas")
	_ = os.MkdirAll(appDir, 0o755)
	a.configPath = filepath.Join(appDir, "config.json")

	if _, err := os.Stat(a.configPath); errors.Is(err, os.ErrNotExist) {
		a.savePlaces(defaultPlaces())
	}
}

func defaultPlaces() []Place {
	return []Place{
		{ID: "51.5074_-0.1278", Name: "London", Admin1: "England", Country: "United Kingdom", Timezone: "Europe/London", Latitude: 51.5074, Longitude: -0.1278},
		{ID: "40.7128_-74.0060", Name: "New York", Admin1: "New York", Country: "United States", Timezone: "America/New_York", Latitude: 40.7128, Longitude: -74.0060},
	}
}

type configFile struct {
	Places []Place `json:"places"`
}

func (a *App) loadPlaces() []Place {
	data, err := os.ReadFile(a.configPath)
	if err != nil {
		return defaultPlaces()
	}
	var cfg configFile
	if err := json.Unmarshal(data, &cfg); err != nil || len(cfg.Places) == 0 {
		return defaultPlaces()
	}
	return cfg.Places
}

func (a *App) savePlaces(places []Place) error {
	data, err := json.MarshalIndent(configFile{Places: places}, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(a.configPath, data, 0o644)
}

func placeID(lat, lon float64) string {
	return fmt.Sprintf("%.4f_%.4f", lat, lon)
}

// GetPlaces returns the currently saved places.
func (a *App) GetPlaces() []Place {
	return a.loadPlaces()
}

// --- Geocoding (Open-Meteo, no API key required) ---

type geocodeAPIResponse struct {
	Results []struct {
		Name      string  `json:"name"`
		Admin1    string  `json:"admin1"`
		Country   string  `json:"country"`
		Timezone  string  `json:"timezone"`
		Latitude  float64 `json:"latitude"`
		Longitude float64 `json:"longitude"`
	} `json:"results"`
}

// cityAliases maps well-known former/historical English city names to the
// name Open-Meteo's geocoder (backed by GeoNames) actually indexes them
// under. GeoNames generally uses a city's current official name, so
// searches for older common names would otherwise return nothing (or an
// unrelated place that happens to share the name).
var cityAliases = []struct {
	alias     string
	canonical string
}{
	{"bangalore", "Bengaluru"},
	{"bombay", "Mumbai"},
	{"calcutta", "Kolkata"},
	{"madras", "Chennai"},
	{"cochin", "Kochi"},
	{"trivandrum", "Thiruvananthapuram"},
	{"poona", "Pune"},
	{"mysore", "Mysuru"},
	{"baroda", "Vadodara"},
	{"peking", "Beijing"},
	{"canton", "Guangzhou"},
	{"saigon", "Ho Chi Minh City"},
	{"rangoon", "Yangon"},
	{"constantinople", "Istanbul"},
	{"christiania", "Oslo"},
	{"danzig", "Gdansk"},
	{"salisbury", "Harare"},
}

// aliasCanonicalFor returns the canonical GeoNames name for a query that
// matches (as a prefix in either direction) a known historical city alias.
func aliasCanonicalFor(query string) (string, bool) {
	q := strings.ToLower(query)
	for _, a := range cityAliases {
		if strings.HasPrefix(a.alias, q) || strings.HasPrefix(q, a.alias) {
			return a.canonical, true
		}
	}
	return "", false
}

// geocode performs a single Open-Meteo geocoding lookup.
func (a *App) geocode(name string) ([]GeocodeResult, error) {
	endpoint := "https://geocoding-api.open-meteo.com/v1/search?" + url.Values{
		"name":     {name},
		"count":    {"8"},
		"language": {"en"},
		"format":   {"json"},
	}.Encode()

	req, err := http.NewRequestWithContext(a.ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	resp, err := a.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("could not reach geocoding service: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("geocoding service returned status %d", resp.StatusCode)
	}

	var parsed geocodeAPIResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		return nil, fmt.Errorf("could not parse geocoding response: %w", err)
	}

	results := make([]GeocodeResult, 0, len(parsed.Results))
	for _, r := range parsed.Results {
		if r.Timezone == "" {
			continue
		}
		results = append(results, GeocodeResult{
			Name:      r.Name,
			Admin1:    r.Admin1,
			Country:   r.Country,
			Timezone:  r.Timezone,
			Latitude:  r.Latitude,
			Longitude: r.Longitude,
		})
	}
	return results, nil
}

// SearchCity looks up candidate cities by free-text name.
func (a *App) SearchCity(query string) ([]GeocodeResult, error) {
	query = strings.TrimSpace(query)
	if query == "" {
		return nil, errors.New("please enter a city name")
	}

	results, err := a.geocode(query)
	if err != nil {
		return nil, err
	}

	// If the query looks like a known historical/alternate name, also
	// search under the current official name and surface those first.
	if canonical, ok := aliasCanonicalFor(query); ok {
		aliasResults, aliasErr := a.geocode(canonical)
		if aliasErr == nil {
			results = mergeGeocodeResults(aliasResults, results)
		}
	}

	if len(results) == 0 {
		return nil, fmt.Errorf("no places found for %q", query)
	}

	return results, nil
}

// mergeGeocodeResults combines two result sets, keeping "first" entries
// ahead of "second" entries and dropping duplicates (same rounded location).
func mergeGeocodeResults(first, second []GeocodeResult) []GeocodeResult {
	seen := make(map[string]bool, len(first)+len(second))
	merged := make([]GeocodeResult, 0, len(first)+len(second))
	for _, list := range [][]GeocodeResult{first, second} {
		for _, r := range list {
			key := placeID(r.Latitude, r.Longitude)
			if seen[key] {
				continue
			}
			seen[key] = true
			merged = append(merged, r)
		}
	}
	return merged
}

// AddPlace saves a new place chosen from search results and returns the updated list.
func (a *App) AddPlace(result GeocodeResult) ([]Place, error) {
	places := a.loadPlaces()

	if len(places) >= maxPlaces {
		return places, fmt.Errorf("you can track at most %d places", maxPlaces)
	}

	id := placeID(result.Latitude, result.Longitude)
	for _, p := range places {
		if p.ID == id {
			return places, fmt.Errorf("%s is already in your list", result.Name)
		}
	}

	places = append(places, Place{
		ID:        id,
		Name:      result.Name,
		Admin1:    result.Admin1,
		Country:   result.Country,
		Timezone:  result.Timezone,
		Latitude:  result.Latitude,
		Longitude: result.Longitude,
	})

	if err := a.savePlaces(places); err != nil {
		return places, err
	}
	return places, nil
}

// RemovePlace deletes a place by ID and returns the updated list.
func (a *App) RemovePlace(id string) ([]Place, error) {
	places := a.loadPlaces()

	if len(places) <= minPlaces {
		return places, fmt.Errorf("you must keep at least %d places", minPlaces)
	}

	filtered := make([]Place, 0, len(places))
	found := false
	for _, p := range places {
		if p.ID == id {
			found = true
			continue
		}
		filtered = append(filtered, p)
	}
	if !found {
		return places, errors.New("place not found")
	}

	if err := a.savePlaces(filtered); err != nil {
		return places, err
	}
	return filtered, nil
}

// --- Weather (Open-Meteo, no API key required) ---

type forecastAPIResponse struct {
	Latitude  float64 `json:"latitude"`
	Longitude float64 `json:"longitude"`
	Current   struct {
		Time                string  `json:"time"`
		Temperature2m       float64 `json:"temperature_2m"`
		RelativeHumidity2m  float64 `json:"relative_humidity_2m"`
		ApparentTemperature float64 `json:"apparent_temperature"`
		IsDay               int     `json:"is_day"`
		WeatherCode         int     `json:"weather_code"`
		WindSpeed10m        float64 `json:"wind_speed_10m"`
	} `json:"current"`
	Daily struct {
		Temperature2mMax []float64 `json:"temperature_2m_max"`
		Temperature2mMin []float64 `json:"temperature_2m_min"`
	} `json:"daily"`
}

// GetPlacesWithWeather returns saved places together with freshly fetched weather,
// ordered as stored (the frontend re-sorts by local time for display).
func (a *App) GetPlacesWithWeather() ([]PlaceWeather, error) {
	places := a.loadPlaces()
	if len(places) == 0 {
		return nil, nil
	}

	lats := make([]string, len(places))
	lons := make([]string, len(places))
	for i, p := range places {
		lats[i] = fmt.Sprintf("%f", p.Latitude)
		lons[i] = fmt.Sprintf("%f", p.Longitude)
	}

	endpoint := "https://api.open-meteo.com/v1/forecast?" + url.Values{
		"latitude":         {strings.Join(lats, ",")},
		"longitude":        {strings.Join(lons, ",")},
		"current":          {"temperature_2m,relative_humidity_2m,apparent_temperature,is_day,weather_code,wind_speed_10m"},
		"daily":            {"temperature_2m_max,temperature_2m_min"},
		"forecast_days":    {"1"},
		"timezone":         {"auto"},
		"temperature_unit": {"celsius"},
		"wind_speed_unit":  {"kmh"},
	}.Encode()

	req, err := http.NewRequestWithContext(a.ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	resp, err := a.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("could not reach weather service: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("weather service returned status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("could not read weather response: %w", err)
	}

	var forecasts []forecastAPIResponse
	trimmed := strings.TrimSpace(string(body))
	if strings.HasPrefix(trimmed, "[") {
		if err := json.Unmarshal(body, &forecasts); err != nil {
			return nil, fmt.Errorf("could not parse weather response: %w", err)
		}
	} else {
		var single forecastAPIResponse
		if err := json.Unmarshal(body, &single); err != nil {
			return nil, fmt.Errorf("could not parse weather response: %w", err)
		}
		forecasts = []forecastAPIResponse{single}
	}

	now := time.Now().Format(time.RFC3339)
	result := make([]PlaceWeather, 0, len(places))
	for i, p := range places {
		pw := PlaceWeather{Place: p}
		if i < len(forecasts) {
			f := forecasts[i]
			w := WeatherData{
				Temperature:  f.Current.Temperature2m,
				ApparentTemp: f.Current.ApparentTemperature,
				Humidity:     f.Current.RelativeHumidity2m,
				WindSpeed:    f.Current.WindSpeed10m,
				WeatherCode:  f.Current.WeatherCode,
				IsDay:        f.Current.IsDay == 1,
				UpdatedAt:    now,
			}
			if len(f.Daily.Temperature2mMax) > 0 {
				w.TodayHigh = f.Daily.Temperature2mMax[0]
			}
			if len(f.Daily.Temperature2mMin) > 0 {
				w.TodayLow = f.Daily.Temperature2mMin[0]
			}
			pw.Weather = w
		}
		result = append(result, pw)
	}

	return result, nil
}

// HourPoint is one hour of today's forecast.
type HourPoint struct {
	Time        string  `json:"time"`
	Temperature float64 `json:"temperature"`
	WeatherCode int     `json:"weatherCode"`
	Humidity    float64 `json:"humidity"`
	WindSpeed   float64 `json:"windSpeed"`
	PrecipProb  float64 `json:"precipProb"`
}

// DayPoint is one day of the multi-day forecast.
type DayPoint struct {
	Date        string  `json:"date"`
	WeatherCode int     `json:"weatherCode"`
	High        float64 `json:"high"`
	Low         float64 `json:"low"`
	PrecipProb  float64 `json:"precipProb"`
}

// DetailedForecast holds an hourly view of today plus a multi-day outlook.
type DetailedForecast struct {
	Timezone string      `json:"timezone"`
	Hourly   []HourPoint `json:"hourly"`
	Daily    []DayPoint  `json:"daily"`
}

type detailedForecastAPIResponse struct {
	Timezone string `json:"timezone"`
	Hourly   struct {
		Time                     []string  `json:"time"`
		Temperature2m            []float64 `json:"temperature_2m"`
		WeatherCode              []int     `json:"weather_code"`
		RelativeHumidity2m       []float64 `json:"relative_humidity_2m"`
		WindSpeed10m             []float64 `json:"wind_speed_10m"`
		PrecipitationProbability []float64 `json:"precipitation_probability"`
	} `json:"hourly"`
	Daily struct {
		Time                        []string  `json:"time"`
		WeatherCode                 []int     `json:"weather_code"`
		Temperature2mMax            []float64 `json:"temperature_2m_max"`
		Temperature2mMin            []float64 `json:"temperature_2m_min"`
		PrecipitationProbabilityMax []float64 `json:"precipitation_probability_max"`
	} `json:"daily"`
}

const detailedForecastDays = 7

// GetDetailedForecast returns an hourly breakdown of today plus a 7-day
// outlook for a single location, used by the weather detail popup.
func (a *App) GetDetailedForecast(lat, lon float64) (DetailedForecast, error) {
	endpoint := "https://api.open-meteo.com/v1/forecast?" + url.Values{
		"latitude":         {fmt.Sprintf("%f", lat)},
		"longitude":        {fmt.Sprintf("%f", lon)},
		"hourly":           {"temperature_2m,weather_code,relative_humidity_2m,wind_speed_10m,precipitation_probability"},
		"daily":            {"weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max"},
		"forecast_days":    {fmt.Sprintf("%d", detailedForecastDays)},
		"timezone":         {"auto"},
		"temperature_unit": {"celsius"},
		"wind_speed_unit":  {"kmh"},
	}.Encode()

	req, err := http.NewRequestWithContext(a.ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return DetailedForecast{}, err
	}
	resp, err := a.httpClient.Do(req)
	if err != nil {
		return DetailedForecast{}, fmt.Errorf("could not reach weather service: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return DetailedForecast{}, fmt.Errorf("weather service returned status %d", resp.StatusCode)
	}

	var f detailedForecastAPIResponse
	if err := json.NewDecoder(resp.Body).Decode(&f); err != nil {
		return DetailedForecast{}, fmt.Errorf("could not parse weather response: %w", err)
	}

	// The hourly series starts at local midnight of today, so the first 24
	// entries are today's hours.
	hourCount := len(f.Hourly.Time)
	if hourCount > 24 {
		hourCount = 24
	}
	hourly := make([]HourPoint, 0, hourCount)
	for i := 0; i < hourCount; i++ {
		hourly = append(hourly, HourPoint{
			Time:        f.Hourly.Time[i],
			Temperature: at(f.Hourly.Temperature2m, i),
			WeatherCode: atInt(f.Hourly.WeatherCode, i),
			Humidity:    at(f.Hourly.RelativeHumidity2m, i),
			WindSpeed:   at(f.Hourly.WindSpeed10m, i),
			PrecipProb:  at(f.Hourly.PrecipitationProbability, i),
		})
	}

	daily := make([]DayPoint, 0, len(f.Daily.Time))
	for i := range f.Daily.Time {
		daily = append(daily, DayPoint{
			Date:        f.Daily.Time[i],
			WeatherCode: atInt(f.Daily.WeatherCode, i),
			High:        at(f.Daily.Temperature2mMax, i),
			Low:         at(f.Daily.Temperature2mMin, i),
			PrecipProb:  at(f.Daily.PrecipitationProbabilityMax, i),
		})
	}

	return DetailedForecast{
		Timezone: f.Timezone,
		Hourly:   hourly,
		Daily:    daily,
	}, nil
}

func at(xs []float64, i int) float64 {
	if i < len(xs) {
		return xs[i]
	}
	return 0
}

func atInt(xs []int, i int) int {
	if i < len(xs) {
		return xs[i]
	}
	return 0
}
