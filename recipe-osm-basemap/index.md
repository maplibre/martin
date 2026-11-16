# Building a basemap from OpenStreetMap

This builds a basemap tileset from an [OpenStreetMap](<https://www.openstreetmap.org/>) extract, with the layers and attributes using the [Shortbread](<https://shortbread-tiles.org/>) schema, so that [VersaTiles styles](<https://github.com/versatiles-org/versatiles-style>) can draw them. The configuration is a port of Planetiler's [`shortbread.yml`](<https://github.com/onthegomap/planetiler/blob/main/planetiler-custommap/src/main/resources/samples/shortbread.yml>), and Martin's tests use the 66 OpenStreetMap examples of its `shortbread.spec.yml`.

> [!IMPORTANT]
>
> **More information on the config used**
>
> The `osm` section is described in [OpenStreetMap PBF Sources](<https://maplibre.org/martin/sources-osm/index.md>).

> [!WARNING]
>
> **Feature is currently not fully implemented**
>
> These are known missing features currently, but are planned:
>
> - `ocean` is omitted since Shortbread takes it from shapefile based coastline data outside OpenStreetMap.
> - `way_area` is left out of `water_polygons`, `water_polygons_labels` and `boundary_labels`, and every `boundary_label` starts at zoom 5. This is because expressions cannot currently read a feature's area.
> - A closed way is a line or a polygon, as its tags make it an [area](<https://maplibre.org/martin/sources-osm/index.md#supported-features-and-handling>). Planetiler reads it as both unless it has `area=yes` or `area=no`, so its `street_polygons` also have closed `highway=pedestrian` ways, and `area:aeroway` ways, which Martin doesn't currently read as lines.
> - `boundary_labels` and `streets_polygons_labels` are points at each polygon's centroid. `pois`, `public_transport` and `addresses` at the centroid of each point, line or polygon. Planetiler takes points and a point on the surface of polygons, or for `addresses` the centroid of convex ones while omitting lines.

1. Download an extract, for example Berlin from [Geofabrik](<https://download.geofabrik.de/>):

   ```bash
   curl -LO https://download.geofabrik.de/europe/germany/berlin-latest.osm.pbf
   ```
2. Save this as `shortbread.yaml`, next to the extract:

   > **shortbread.yaml**
   >
   > ```yaml
   > osm:
   >   sources:
   >     shortbread:
   >       path: berlin-latest.osm.pbf
   >       maxzoom: 14
   >       tags:
   >         - name
   >         - "name:en"
   >         - "name:de"
   >         - "addr:housename"
   >         - "addr:housenumber"
   >         - admin_level
   >         - aerialway
   >         - aeroway
   >         - amenity
   >         - "area:aeroway"
   >         - atm
   >         - bicycle
   >         - boundary
   >         - bridge
   >         - building
   >         - capital
   >         - covered
   >         - cuisine
   >         - denomination
   >         - disputed
   >         - emergency
   >         - highway
   >         - historic
   >         - horse
   >         - iata
   >         - information
   >         - landuse
   >         - leisure
   >         - man_made
   >         - maritime
   >         - military
   >         - motor_vehicle
   >         - natural
   >         - office
   >         - oneway
   >         - place
   >         - population
   >         - railway
   >         - "recycling:clothes"
   >         - "recycling:glass_bottles"
   >         - "recycling:paper"
   >         - "recycling:scrap_metal"
   >         - ref
   >         - religion
   >         - route
   >         - service
   >         - shop
   >         - sport
   >         - surface
   >         - tourism
   >         - "tower:type"
   >         - tracktype
   >         - tunnel
   >         - vending
   >         - waterway
   >         - wetland
   >       layers:
   >         water_polygons:
   >           geometry: polygon
   >           minzoom: "waterway in ['dock', 'canal'] ? 10 : 4"
   >           where: >-
   >             natural in ['glacier', 'water'] || waterway in ['riverbank', 'dock', 'canal'] || landuse in ['reservoir',
   >             'basin']
   >           attributes:
   >             kind: >-
   >               natural in ['glacier', 'water'] ? natural : waterway in ['riverbank', 'dock', 'canal'] ? waterway :
   >               landuse
   >         water_polygons_labels:
   >           geometry: polygon_centroid
   >           minzoom: "waterway in ['dock', 'canal'] ? 10 : 14"
   >           where: >-
   >             name != null && (natural in ['glacier', 'water'] || waterway in ['riverbank', 'dock', 'canal'] || landuse
   >             in ['reservoir', 'basin'])
   >           attributes:
   >             kind: >-
   >               natural in ['glacier', 'water'] ? natural : waterway in ['riverbank', 'dock', 'canal'] ? waterway :
   >               landuse
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >         water_lines:
   >           geometry: line
   >           minzoom: "waterway in ['stream', 'ditch'] ? 14 : 9"
   >           where: "waterway in ['canal', 'river', 'stream', 'ditch']"
   >           attributes:
   >             kind: waterway
   >             tunnel:
   >               expr: "tunnel in ['yes', 'building_passage'] || covered == 'yes'"
   >               minzoom: 11
   >             bridge:
   >               expr: >-
   >                 bridge in ['yes', 'viaduct', 'boardwalk', 'cantilever', 'covered', 'low_water_crossing', 'movable',
   >                 'trestle']
   >               minzoom: 11
   >         water_lines_labels:
   >           geometry: line
   >           minzoom: "waterway in ['stream', 'ditch'] ? 14 : 12"
   >           where: "name != null && waterway in ['canal', 'river', 'stream', 'ditch']"
   >           attributes:
   >             kind: waterway
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >             tunnel: "tunnel in ['yes', 'building_passage'] || covered == 'yes'"
   >             bridge: >-
   >               bridge in ['yes', 'viaduct', 'boardwalk', 'cantilever', 'covered', 'low_water_crossing', 'movable',
   >               'trestle']
   >         dam_lines:
   >           geometry: line
   >           minzoom: 12
   >           where: "waterway == 'dam'"
   >           attributes:
   >             kind: { value: dam }
   >         dam_polygons:
   >           geometry: polygon
   >           minzoom: 12
   >           where: "waterway == 'dam'"
   >           attributes:
   >             kind: { value: dam }
   >         pier_lines:
   >           geometry: line
   >           minzoom: 12
   >           where: "man_made in ['pier', 'breakwater', 'groyne']"
   >           attributes:
   >             kind: man_made
   >         pier_polygons:
   >           geometry: polygon
   >           minzoom: 12
   >           where: "man_made in ['pier', 'breakwater', 'groyne']"
   >           attributes:
   >             kind: man_made
   >         boundaries:
   >           geometry: line
   >           min_size: 0
   >           minzoom: "admin_level == '2' ? 0 : 7"
   >           where: "boundary == 'administrative' && admin_level in ['2', '4']"
   >           attributes:
   >             maritime: "maritime != null || natural == 'coastline'"
   >             admin_level: "int(admin_level)"
   >             disputed: "disputed == 'yes'"
   >         boundary_labels:
   >           geometry: polygon_centroid
   >           minzoom: 5
   >           where: "boundary == 'administrative' && admin_level in ['2', '4']"
   >           attributes:
   >             admin_level: "int(admin_level)"
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >         place_labels:
   >           geometry: point
   >           minzoom: >-
   >             place in ['city', 'town', 'village', 'hamlet'] && capital in ['yes', '4'] ? 4 : place == 'city' ? 6 :
   >             place == 'town' ? 7 : 10
   >           where: >-
   >             name != null && place in ['city', 'town', 'village', 'hamlet', 'suburb', 'quarter', 'neighbourhood',
   >             'isolated_dwelling', 'farm', 'island', 'locality']
   >           attributes:
   >             kind: >-
   >               place in ['city', 'town', 'village', 'hamlet'] && capital == 'yes' ? 'capital' : place in ['city',
   >               'town', 'village', 'hamlet'] && capital == '4' ? 'state_capital' : place
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >             population: >-
   >               population != null ? int(double(population.replace(',', ''))) : place == 'city' ? 100000 : place ==
   >               'town' ? 5000 : place == 'suburb' ? 1000 : place == 'quarter' ? 500 : place == 'village' ? 100 : place
   >               == 'hamlet' ? 50 : place in ['isolated_dwelling', 'farm'] ? 5 : 0
   >         land:
   >           geometry: polygon
   >           minzoom: >-
   >             natural == 'wood' || landuse == 'forest' ? 7 : landuse in ['brownfield', 'commercial', 'farmland',
   >             'farmyard', 'garages', 'greenfield', 'industrial', 'landfill', 'railway', 'residential', 'retail'] ||
   >             natural in ['beach', 'sand'] ? 10 : amenity == 'grave_yard' || landuse == 'cemetery' ? 13 : 11
   >           where: >-
   >             amenity == 'grave_yard' || landuse in ['allotments', 'brownfield', 'cemetery', 'commercial', 'farmland',
   >             'farmyard', 'forest', 'garages', 'grass', 'greenfield', 'greenhouse_horticulture', 'industrial',
   >             'landfill', 'meadow', 'orchard', 'plant_nursery', 'quarry', 'railway', 'recreation_ground', 'residential',
   >             'retail', 'village_green', 'vineyard'] || leisure in ['garden', 'golf_course', 'miniature_golf', 'park',
   >             'playground'] || natural in ['bare_rock', 'beach', 'grassland', 'heath', 'sand', 'scree', 'scrub',
   >             'shingle', 'wood'] || wetland in ['bog', 'marsh', 'string_bog', 'swamp', 'wet_meadow']
   >           attributes:
   >             kind: >-
   >               amenity == 'grave_yard' ? amenity : landuse in ['allotments', 'brownfield', 'cemetery', 'commercial',
   >               'farmland', 'farmyard', 'forest', 'garages', 'grass', 'greenfield', 'greenhouse_horticulture',
   >               'industrial', 'landfill', 'meadow', 'orchard', 'plant_nursery', 'quarry', 'railway',
   >               'recreation_ground', 'residential', 'retail', 'village_green', 'vineyard'] ? landuse : leisure in
   >               ['garden', 'golf_course', 'miniature_golf', 'park', 'playground'] ? leisure : natural == 'wood' ?
   >               'forest' : natural in ['bare_rock', 'beach', 'grassland', 'heath', 'sand', 'scree', 'scrub', 'shingle',
   >               'wood'] ? natural : wetland
   >         sites:
   >           geometry: polygon
   >           minzoom: 14
   >           where: >-
   >             military == 'danger_area' || leisure == 'sports_center' || landuse == 'construction' || amenity in
   >             ['bicycle_parking', 'college', 'hospital', 'parking', 'prison', 'university']
   >           attributes:
   >             kind: >-
   >               military == 'danger_area' ? military : leisure == 'sports_center' ? leisure : landuse == 'construction'
   >               ? landuse : amenity
   >         buildings:
   >           geometry: polygon
   >           minzoom: 14
   >           where: "building != null && building != 'no'"
   >           attributes: []
   >         addresses:
   >           geometry: centroid
   >           minzoom: 14
   >           where: >-
   >             (feature['addr:housenumber'] != null || feature['addr:housename'] != null) && !(amenity in ['arts_centre',
   >             'atm', 'bank', 'bar', 'bench', 'bicycle_rental', 'biergarten', 'cafe', 'car_rental', 'car_sharing',
   >             'car_wash', 'cinema', 'clinic', 'college', 'community_centre', 'courthouse', 'dentist', 'doctors',
   >             'dog_park', 'drinking_water', 'embassy', 'fast_food', 'fire_station', 'food_court', 'fountain',
   >             'grave_yard', 'hospital', 'hunting_stand', 'library', 'marketplace', 'nightclub', 'nursing_home',
   >             'pharmacy', 'place_of_worship', 'playground', 'police', 'post_box', 'post_office', 'prison', 'pub',
   >             'public_building', 'recycling', 'restaurant', 'school', 'shelter', 'telephone', 'theatre', 'toilets',
   >             'townhall', 'university', 'vending_machine', 'veterinary', 'waste_basket'] || emergency in
   >             ['defibrillator', 'fire_hydrant', 'phone'] || highway == 'emergency_access_point' || historic in
   >             ['archaeological_site', 'battlefield', 'castle', 'fort', 'memorial', 'monument', 'ruins', 'wayside_cross',
   >             'wayside_shrine'] || leisure in ['golf_course', 'ice_rink', 'pitch', 'sports_centre', 'stadium',
   >             'swimming_pool', 'water_park'] || man_made in ['lighthouse', 'surveillance', 'tower', 'wastewater_plant',
   >             'water_well', 'water_works', 'watermill', 'windmill'] || office == 'diplomatic' || shop in ['alcohol',
   >             'bakery', 'beauty', 'beverages', 'bicycle', 'books', 'butcher', 'car', 'chemist', 'clothes', 'computer',
   >             'convenience', 'department_store', 'doityourself', 'dry_cleaning', 'florist', 'furniture',
   >             'garden_centre', 'general', 'gift', 'greengrocer', 'hairdresser', 'hardware', 'jewelry', 'kiosk',
   >             'laundry', 'mall', 'mobile_phone', 'newsagent', 'optician', 'outdoor', 'shoes', 'sports', 'stationery',
   >             'supermarket', 'toys', 'travel_agency', 'video'] || tourism in ['artwork', 'alpine_hut',
   >             'bed_and_breakfast', 'camp_site', 'caravan_site', 'chalet', 'guest_house', 'hostel', 'hotel',
   >             'information', 'motel', 'picnic_site', 'theme_park', 'viewpoint', 'zoo'])
   >           attributes:
   >             housename: "feature['addr:housename']"
   >             housenumber: "feature['addr:housenumber']"
   >         streets:
   >           geometry: line
   >           min_size: 0
   >           minzoom: >-
   >             highway != null && highway.startsWith('motorway') ? 5 : highway != null && highway.startsWith('trunk') ? 6
   >             : highway != null && highway.startsWith('primary') || railway in ['rail', 'narrow_gauge'] && service !=
   >             null ? 8 : highway != null && highway.startsWith('secondary') ? 9 : railway in ['rail', 'narrow_gauge'] &&
   >             service == null || railway in ['funicular', 'light_rail', 'monorail', 'subway', 'tram'] || highway ==
   >             'tertiary' ? 10 : aeroway == 'runway' ? 11 : highway in ['bus_guideway', 'busway', 'residential',
   >             'unclassified'] ? 12 : 13
   >           where: >-
   >             highway in ['motorway', 'motorway_link', 'trunk', 'trunk_link', 'primary', 'primary_link', 'secondary',
   >             'secondary_link', 'tertiary', 'tertiary_link', 'unclassified', 'residential', 'busway', 'bus_guideway',
   >             'living_street', 'service', 'pedestrian', 'track', 'footway', 'steps', 'path', 'cycleway'] || aeroway in
   >             ['runway', 'taxiway'] || railway in ['rail', 'narrow_gauge', 'tram', 'light_rail', 'funicular', 'subway',
   >             'monorail']
   >           attributes:
   >             kind: >-
   >               (highway in ['motorway', 'motorway_link', 'trunk', 'trunk_link', 'primary', 'primary_link', 'secondary',
   >               'secondary_link', 'tertiary', 'tertiary_link', 'unclassified', 'residential', 'busway', 'bus_guideway',
   >               'living_street', 'service', 'pedestrian', 'track', 'footway', 'steps', 'path', 'cycleway'] ? highway :
   >               aeroway in ['runway', 'taxiway'] ? aeroway : railway).replace('_link', '')
   >             link:
   >               expr: "highway in ['motorway_link', 'trunk_link', 'primary_link', 'secondary_link', 'tertiary_link']"
   >               minzoom: 11
   >             rail:
   >               expr: "railway != null"
   >               minzoom: 5
   >             tunnel:
   >               expr: "tunnel in ['yes', 'building_passage'] || covered == 'yes'"
   >               minzoom: 11
   >             bridge:
   >               expr: >-
   >                 bridge in ['yes', 'viaduct', 'boardwalk', 'cantilever', 'covered', 'low_water_crossing', 'movable',
   >                 'trestle']
   >               minzoom: 11
   >             oneway:
   >               expr: "railway == null && oneway in ['yes', '1', 'true', '-1']"
   >               minzoom: 14
   >             oneway_reverse:
   >               expr: "railway == null && oneway == '-1'"
   >               minzoom: 14
   >             tracktype:
   >               expr: tracktype
   >               minzoom: 11
   >             surface:
   >               expr: surface
   >               minzoom: 11
   >             service:
   >               expr: service
   >               minzoom: 11
   >             bicycle:
   >               expr: bicycle
   >               minzoom: 14
   >             horse:
   >               expr: horse
   >               minzoom: 14
   >         street_polygons:
   >           geometry: polygon
   >           minzoom: "feature['area:aeroway'] == 'runway' ? 11 : feature['area:aeroway'] == 'taxiway' ? 13 : 14"
   >           where: "feature['area:aeroway'] in ['runway', 'taxiway'] || highway in ['pedestrian', 'service']"
   >           attributes:
   >             kind: "feature['area:aeroway'] in ['runway', 'taxiway'] ? feature['area:aeroway'] : highway"
   >             bridge: >-
   >               bridge in ['yes', 'viaduct', 'boardwalk', 'cantilever', 'covered', 'low_water_crossing', 'movable',
   >               'trestle']
   >             rail: { value: false }
   >             service: service
   >             surface: surface
   >             tunnel: "tunnel in ['yes', 'building_passage'] || covered == 'yes'"
   >         street_labels:
   >           geometry: line
   >           minzoom: >-
   >             highway == 'motorway' || railway in ['rail', 'narrow_gauge', 'tram', 'light_rail', 'funicular', 'subway',
   >             'monorail'] ? 10 : aeroway == 'runway' ? 11 : highway in ['trunk', 'primary'] ? 12 : highway in
   >             ['motorway_link', 'trunk_link', 'primary_link', 'secondary', 'secondary_link', 'tertiary'] || aeroway ==
   >             'taxiway' ? 13 : 14
   >           where: >-
   >             (name != null || ref != null) && (highway in ['motorway', 'motorway_link', 'trunk', 'trunk_link',
   >             'primary', 'primary_link', 'secondary', 'secondary_link', 'tertiary', 'tertiary_link', 'unclassified',
   >             'residential', 'busway', 'bus_guideway', 'living_street', 'service', 'pedestrian', 'track', 'footway',
   >             'steps', 'path', 'cycleway'] || aeroway in ['runway', 'taxiway'] || railway in ['rail', 'narrow_gauge',
   >             'tram', 'light_rail', 'funicular', 'subway', 'monorail'])
   >           attributes:
   >             kind: >-
   >               highway in ['motorway', 'motorway_link', 'trunk', 'trunk_link', 'primary', 'primary_link', 'secondary',
   >               'secondary_link', 'tertiary', 'tertiary_link', 'unclassified', 'residential', 'busway', 'bus_guideway',
   >               'living_street', 'service', 'pedestrian', 'track', 'footway', 'steps', 'path', 'cycleway'] ? highway :
   >               aeroway in ['runway', 'taxiway'] ? aeroway : railway
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >             ref: "ref != null ? ref.replace(';', '\\n') : null"
   >             ref_cols: "ref != null ? max(ref.split(';').map(r, size(r))) : null"
   >             ref_rows: "ref != null ? size(ref.split(';')) : null"
   >             tunnel: "tunnel in ['yes', 'building_passage'] || covered == 'yes'"
   >         streets_polygons_labels:
   >           geometry: polygon_centroid
   >           minzoom: 14
   >           where: "name != null && (aeroway in ['runway', 'taxiway'] || highway in ['pedestrian', 'service'])"
   >           attributes:
   >             kind: "aeroway in ['runway', 'taxiway'] ? aeroway : highway"
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >         street_labels_points:
   >           geometry: point
   >           minzoom: 12
   >           where: "highway == 'motorway_junction'"
   >           attributes:
   >             kind: highway
   >             ref: ref
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >         bridges:
   >           geometry: polygon
   >           minzoom: 12
   >           where: "man_made == 'bridge'"
   >           attributes:
   >             kind: { value: bridge }
   >         aerialways:
   >           geometry: line
   >           minzoom: 12
   >           where: >-
   >             aerialway in ['cable_car', 'gondola', 'goods', 'chair_lift', 'drag_lift', 't-bar', 'j-bar', 'platter',
   >             'rope_tow']
   >           attributes:
   >             kind: aerialway
   >         ferries:
   >           geometry: line
   >           minzoom: "motor_vehicle == 'no' ? 12 : 10"
   >           where: "route == 'ferry'"
   >           attributes:
   >             kind: route
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >         public_transport:
   >           geometry: centroid
   >           minzoom: >-
   >             aeroway == 'aerodrome' ? 11 : amenity == 'ferry_terminal' ? 12 : railway in ['station', 'halt'] ||
   >             aerialway == 'station' || aeroway == 'helipad' || amenity == 'bus_station' ? 13 : 14
   >           where: >-
   >             aerialway == 'station' || aeroway in ['aerodrome', 'helipad'] || amenity in ['bus_station',
   >             'ferry_terminal'] || highway == 'bus_stop' || railway in ['station', 'halt', 'tram_stop']
   >           attributes:
   >             kind: >-
   >               aeroway in ['aerodrome', 'helipad'] ? aeroway : amenity in ['bus_station', 'ferry_terminal'] ? amenity :
   >               railway in ['station', 'halt', 'tram_stop'] ? railway : highway == 'bus_stop' ? highway :
   >               'aerialway_station'
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >             iata: iata
   >         pois:
   >           geometry: centroid
   >           minzoom: 14
   >           where: >-
   >             amenity in ['arts_centre', 'atm', 'bank', 'bar', 'bench', 'bicycle_rental', 'biergarten', 'cafe',
   >             'car_rental', 'car_sharing', 'car_wash', 'cinema', 'clinic', 'college', 'community_centre', 'courthouse',
   >             'dentist', 'doctors', 'dog_park', 'drinking_water', 'embassy', 'fast_food', 'fire_station', 'food_court',
   >             'fountain', 'grave_yard', 'hospital', 'hunting_stand', 'library', 'marketplace', 'nightclub',
   >             'nursing_home', 'pharmacy', 'place_of_worship', 'playground', 'police', 'post_box', 'post_office',
   >             'prison', 'pub', 'public_building', 'recycling', 'restaurant', 'school', 'shelter', 'telephone',
   >             'theatre', 'toilets', 'townhall', 'university', 'vending_machine', 'veterinary', 'waste_basket'] ||
   >             emergency in ['defibrillator', 'fire_hydrant', 'phone'] || highway == 'emergency_access_point' || historic
   >             in ['archaeological_site', 'battlefield', 'castle', 'fort', 'memorial', 'monument', 'ruins',
   >             'wayside_cross', 'wayside_shrine'] || leisure in ['golf_course', 'ice_rink', 'pitch', 'sports_centre',
   >             'stadium', 'swimming_pool', 'water_park'] || man_made in ['lighthouse', 'surveillance', 'tower',
   >             'wastewater_plant', 'water_well', 'water_works', 'watermill', 'windmill'] || office == 'diplomatic' ||
   >             shop in ['alcohol', 'bakery', 'beauty', 'beverages', 'bicycle', 'books', 'butcher', 'car', 'chemist',
   >             'clothes', 'computer', 'convenience', 'department_store', 'doityourself', 'dry_cleaning', 'florist',
   >             'furniture', 'garden_centre', 'general', 'gift', 'greengrocer', 'hairdresser', 'hardware', 'jewelry',
   >             'kiosk', 'laundry', 'mall', 'mobile_phone', 'newsagent', 'optician', 'outdoor', 'shoes', 'sports',
   >             'stationery', 'supermarket', 'toys', 'travel_agency', 'video'] || tourism in ['artwork', 'alpine_hut',
   >             'bed_and_breakfast', 'camp_site', 'caravan_site', 'chalet', 'guest_house', 'hostel', 'hotel',
   >             'information', 'motel', 'picnic_site', 'theme_park', 'viewpoint', 'zoo']
   >           attributes:
   >             amenity: >-
   >               amenity in ['arts_centre', 'atm', 'bank', 'bar', 'bench', 'bicycle_rental', 'biergarten', 'cafe',
   >               'car_rental', 'car_sharing', 'car_wash', 'cinema', 'clinic', 'college', 'community_centre',
   >               'courthouse', 'dentist', 'doctors', 'dog_park', 'drinking_water', 'embassy', 'fast_food',
   >               'fire_station', 'food_court', 'fountain', 'grave_yard', 'hospital', 'hunting_stand', 'library',
   >               'marketplace', 'nightclub', 'nursing_home', 'pharmacy', 'place_of_worship', 'playground', 'police',
   >               'post_box', 'post_office', 'prison', 'pub', 'public_building', 'recycling', 'restaurant', 'school',
   >               'shelter', 'telephone', 'theatre', 'toilets', 'townhall', 'university', 'vending_machine', 'veterinary',
   >               'waste_basket'] ? amenity : null
   >             leisure: >-
   >               leisure in ['golf_course', 'ice_rink', 'pitch', 'sports_centre', 'stadium', 'swimming_pool',
   >               'water_park'] ? leisure : null
   >             tourism: >-
   >               tourism in ['artwork', 'alpine_hut', 'bed_and_breakfast', 'camp_site', 'caravan_site', 'chalet',
   >               'guest_house', 'hostel', 'hotel', 'information', 'motel', 'picnic_site', 'theme_park', 'viewpoint',
   >               'zoo'] ? tourism : null
   >             shop: >-
   >               shop in ['alcohol', 'bakery', 'beauty', 'beverages', 'bicycle', 'books', 'butcher', 'car', 'chemist',
   >               'clothes', 'computer', 'convenience', 'department_store', 'doityourself', 'dry_cleaning', 'florist',
   >               'furniture', 'garden_centre', 'general', 'gift', 'greengrocer', 'hairdresser', 'hardware', 'jewelry',
   >               'kiosk', 'laundry', 'mall', 'mobile_phone', 'newsagent', 'optician', 'outdoor', 'shoes', 'sports',
   >               'stationery', 'supermarket', 'toys', 'travel_agency', 'video'] ? shop : null
   >             man_made: >-
   >               man_made in ['lighthouse', 'surveillance', 'tower', 'wastewater_plant', 'water_well', 'water_works',
   >               'watermill', 'windmill'] ? man_made : null
   >             historic: >-
   >               historic in ['archaeological_site', 'battlefield', 'castle', 'fort', 'memorial', 'monument', 'ruins',
   >               'wayside_cross', 'wayside_shrine'] ? historic : null
   >             emergency: "emergency in ['defibrillator', 'fire_hydrant', 'phone'] ? emergency : null"
   >             highway: "highway == 'emergency_access_point' ? highway : null"
   >             office: "office == 'diplomatic' ? office : null"
   >             name: name
   >             name_en: "feature['name:en']"
   >             name_de: "feature['name:de']"
   >             housename: "feature['addr:housename']"
   >             housenumber: "feature['addr:housenumber']"
   >             cuisine: "amenity in ['restaurant', 'fast_food', 'pub', 'bar', 'cafe'] ? cuisine : null"
   >             sport: "leisure in ['pitch', 'sports_centre'] ? sport : null"
   >             vending: "amenity == 'vending_machine' ? vending : null"
   >             information: "tourism == 'information' ? information : null"
   >             "tower:type": "man_made == 'tower' ? feature['tower:type'] : null"
   >             religion: "amenity == 'place_of_worship' ? religion : null"
   >             denomination: "amenity == 'place_of_worship' ? denomination : null"
   >             "recycling:glass_bottles": "amenity == 'recycling' ? feature['recycling:glass_bottles'] == 'yes' : null"
   >             "recycling:paper": "amenity == 'recycling' ? feature['recycling:paper'] == 'yes' : null"
   >             "recycling:clothes": "amenity == 'recycling' ? feature['recycling:clothes'] == 'yes' : null"
   >             "recycling:scrap_metal": "amenity == 'recycling' ? feature['recycling:scrap_metal'] == 'yes' : null"
   >             atm: "amenity == 'bank' ? atm == 'yes' : null"
   > ```
3. Generate the tileset:

   ```bash
   martin cp --mode bulk --config shortbread.yaml --output-file berlin.mbtiles --format mvt --max-zoom 14
   ```
4. Serve it, and point a style's `versatiles-shortbread` source at `http://localhost:3000/berlin`:

   ```bash
   martin berlin.mbtiles
   ```

| Extract | PBF | Features | Tiles | Time | Memory |
| --- | --- | --- | --- | --- | --- |
| Berlin, 2026-11-11 | 100 MB | 1,631,630 | 914 | 3.1 s | 2.7 GB |
| Germany, 2026-11-11 | 4.9 GB | 75,134,826 | 228,018 | 3m 9s | 18.2 GB |

Measured on 24 threads, with the extract in the page cache.
