(ns relational-engine.show
  (:require [clojure.java.io])
  (:import (com.github.freva.asciitable AsciiTable)
           (com.github.freva.asciitable Column)))

(let* [data (to-array-2d [["1" "2"]
                          ["1" "2"]
                          ["1" "2"]])
       table-internal1 (AsciiTable/getTable data)
       data2 (to-array-2d [["1" "2"]
                          ["1" "2"]
                          ["1" table-internal1]])
       table-internal2 (AsciiTable/getTable data2)
       data3 (to-array-2d [["1" "2" "3"]
                           ["1" "2" "3"]
                           ["1" "2" table-internal2]])
       x (AsciiTable/getTable data3)]
      (println x))

(defrecord Person [fname lname address])
(defrecord Address [street num])

(let [data [(Person. "Stu" "Halloway" (Address. "Saint James St." 123))
            (Person. "Marcos" "Magueta" (Address. "Saint Jerome St." 203))
            (Person. "Alfred" "Parker" (Address. "Saint Stephen St." 842))]
      java-cast (comp java.util.Arrays/asList to-array)]
  (-> (AsciiTable/getTable
       AsciiTable/FANCY_ASCII
       (java-cast data)
       (-> [(.with (.header (new Column) "fname:String") :fname)
            (.with (.header (new Column) "lname:String") :lname)
            (.with (.header (new Column) "Address:String")
                   (fn [x] (AsciiTable/getTable
                            (java.util.Arrays/asList (to-array [(:address x)]))
                            (-> [(.with (.header (new Column) "street:String")
                                        :street)
                                 (.with (.header (new Column) "num:Integer")
                                        (comp str :num))]
                                java-cast))))]
           java-cast))
      println))
