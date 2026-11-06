# Third-party notices

## GeoNames city data

`frontend/data/cities.txt` is derived from the GeoNames `cities15000` extract
(<https://download.geonames.org/export/dump/>): cities with a population above 15,000, reduced to
name, country, US state and coordinates rounded to two decimals. It is used only offline, to turn a
position into the nearest city name on the user's own computer.

Data from [GeoNames](https://www.geonames.org/), licensed under
[Creative Commons Attribution 4.0](https://creativecommons.org/licenses/by/4.0/). The reduced file
was produced by `frontend/scripts/build-cities.mjs`; it is modified from the original.
